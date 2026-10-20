"""Kullanıcıya özel dosya kuyruğu üzerinden LumiaDAM ile 3ds Max bağlantısı."""
import hashlib
import json
import ntpath
import os
import re
import shutil
import stat
import subprocess
import threading
import time
import uuid
import zipfile
from concurrent.futures import ThreadPoolExecutor
from contextlib import contextmanager
from pathlib import Path
import zlib

MAX_BYTES = 20 * 1024 ** 3
MAX_ENTRIES = 100000
PROTOCOL_VERSION = 1
MERGE_SUFFIXES = (".max", ".zip", ".rar")

TR = {
    "LumiaDAM — Convert V-Ray to Corona?": "LumiaDAM — V-Ray, Corona biçimine dönüştürülsün mü?",
    "This model uses V-Ray materials or lights. Convert the imported objects to Corona?\n\n"
    "Yes: use the installed Corona Converter for imported objects only.\n"
    "No: keep the source materials and lights, like a normal Max merge.\n\n"
    "The current renderer, environment, and existing scene objects will be preserved.":
    "Bu model V-Ray materyalleri veya ışıkları kullanıyor. İçe aktarılan nesneler Corona biçimine dönüştürülsün mü?\n\n"
    "Evet: Kurulu Corona Converter yalnız içe aktarılan nesneler için kullanılır.\n"
    "Hayır: Normal Max birleştirmesinde olduğu gibi kaynak materyaller ve ışıklar korunur.\n\n"
    "Mevcut render motoru, ortam ve sahnedeki diğer nesneler korunacaktır.",
    "\n\nSome source plugin definitions are missing. Conversion will be checked; unreadable source materials cannot be reconstructed automatically.":
    "\n\nBazı kaynak eklenti tanımları eksik. Dönüşüm kontrol edilecek; okunamayan kaynak materyaller otomatik olarak yeniden oluşturulamaz.",
    "Yes": "Evet", "No": "Hayır", "OK": "Tamam", "Cancel": "İptal",
    "LumiaDAM — Select model": "LumiaDAM — Model seç",
    "MAX model:": "MAX modeli:",
    "Choose the MAX file to merge:": "Birleştirilecek MAX dosyasını seçin:",
    "object": "nesne",
    "objects": "nesne",
    "missing plugin": "eksik eklenti", "missing plugins": "eksik eklenti",
    "current renderer": "etkin render motoru",
    "Unknown renderer": "Bilinmeyen render motoru",
    "Incompatible or unreadable MAX files were skipped:": "Uyumsuz veya okunamayan MAX dosyaları atlandı:",
    "No readable MAX models were found in the archive.": "Arşivde okunabilir MAX modeli bulunamadı.",
}


def _language():
    # Her diyalog açılışında oku; Max yeniden başlatılmadan dil değişikliği uygulanır.
    root = os.environ.get("LOCALAPPDATA")
    if not root:
        return "en"
    try:
        with (Path(root) / "LumiaDAM/preferences.json").open(encoding="utf-8-sig") as reader:
            preferences = json.loads(reader.read(16385))
        return "tr" if isinstance(preferences, dict) and preferences.get("language") == "tr" else "en"
    except (OSError, ValueError, UnicodeError):
        return "en"


def _t(message, language):
    return TR.get(message, message) if language == "tr" else message


def validate_request(request):
    if request.get("version") != PROTOCOL_VERSION or request.get("action") != "merge":
        raise ValueError("Unsupported LumiaDAM bridge request.")
    uuid.UUID(request["id"])
    if time.time() - request.get("createdAt", 0) > 60:
        raise ValueError("Merge request expired. Select the asset again in LumiaDAM.")
    if Path(request["sourcePath"]).suffix.lower() not in MERGE_SUFFIXES:
        raise ValueError("Only MAX files and ZIP/RAR archives are supported.")


def _archive_path(name):
    name = name.replace("\\", "/")
    parts = name.rstrip("/").split("/")
    if not name or ntpath.splitdrive(name)[0] or name.startswith("/"):
        raise ValueError("Archive contains an absolute path.")
    for part in parts:
        if part in ("", ".", "..") or part.endswith((".", " ")) or re.search(r'[<>:"|?*\x00-\x1f]', part):
            raise ValueError("Archive contains an unsafe file path.")
        if re.match(r"^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\.|$)", part, re.I):
            raise ValueError("Archive contains a reserved Windows filename.")
    return Path(*parts)


def _zip_path(info):
    path = _archive_path(info.filename)
    if stat.S_ISLNK(info.external_attr >> 16) or info.flag_bits & 1:
        raise ValueError("Symbolic links and encrypted ZIP files are not supported.")
    return path


def _unrar_executable():
    # Kurulu WinRAR'ın ücretsiz UnRAR CLI'ını kullan; başka bir paket veya servis kurma.
    candidates = [Path(os.environ[variable]) / "WinRAR/UnRAR.exe"
                  for variable in ("ProgramFiles", "ProgramFiles(x86)") if os.environ.get(variable)]
    located = shutil.which("UnRAR.exe") or shutil.which("unrar")
    if located:
        candidates.append(Path(located))
    for executable in candidates:
        if executable.is_file():
            return executable
    raise ValueError("RAR merge requires UnRAR.exe from WinRAR or the free standalone UnRAR tool. Install it and restart 3ds Max.")


@contextmanager
def _rar_output(executable, command, source):
    flags = ["-inul"] if command == "p" else ["-c-", "-scf"]
    process = subprocess.Popen([str(executable), command, *flags, "-cfg-", "-p-", "--", str(source)],
                               stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                               creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
    expired = threading.Event()

    def timeout():
        expired.set()
        try:
            process.kill()
        except OSError:
            pass

    # read() de bloklanabilir; süre sınırı yalnız process.wait üzerinde bırakılmaz.
    watchdog = threading.Timer(30 if command == "lt" else 1800, timeout)
    watchdog.daemon = True
    watchdog.start()
    try:
        yield process.stdout
        result = process.wait(timeout=5)
        if expired.is_set():
            raise ValueError("RAR processing timed out. Check archive availability and try again.")
        if result != 0:
            raise ValueError(f"Could not read RAR archive (UnRAR code {result}). It may be damaged, encrypted, or missing a volume.")
    finally:
        watchdog.cancel()
        if process.poll() is None:
            process.kill()
        process.stdout.close()
        process.wait()


def _rar_entries(listing):
    records, current = [], None
    fields = {"Name", "Type", "Size", "Packed size", "Ratio", "Modified", "Created", "Accessed",
              "mtime", "ctime", "atime", "Attributes", "CRC32", "CRC32 MAC", "BLAKE2", "BLAKE2 MAC", "Host OS", "Compression", "Flags", "Pack-CRC32", "Target"}
    for line in listing.split("\n"):
        line = line[:-1] if line.endswith("\r") else line
        if not line.strip():
            if current is not None:
                records.append(current)
                current = None
            continue
        key, separator, value = line.partition(":")
        key, value = key.strip(), value[1:] if value.startswith(" ") else value
        if current is None and key == "Details" and "volume" in value.lower():
            raise ValueError("Multipart RAR archives are not supported. Extract the complete archive first and select its MAX file.")
        if current is None and key != "Name":
            continue
        if current is None:
            current = {}
        # Devam satırları veya ikinci Name alanı, dosya adını kırparak başka hedefe yazamaz.
        if not separator or key not in fields or key in current:
            raise ValueError("RAR archive contains unsupported or ambiguous metadata.")
        current[key] = value
    if current is not None:
        records.append(current)
    if not records or len(records) > MAX_ENTRIES:
        raise ValueError("RAR is empty or exceeds the 100,000-entry extraction limit.")
    entries, seen, total = [], set(), 0
    for record in records:
        if record.get("Type") not in ("File", "Directory") or "Target" in record:
            raise ValueError("Links and special archive entries are not supported.")
        if "encrypted" in record.get("Flags", "").lower():
            raise ValueError("Password-protected RAR archives are not supported.")
        if "Pack-CRC32" in record:
            raise ValueError("Multipart RAR archives are not supported. Extract them first and select the MAX file.")
        path = _archive_path(record["Name"])
        normalized = str(path).casefold()
        if normalized in seen:
            raise ValueError("RAR contains duplicate Windows file paths.")
        seen.add(normalized)
        directory = record["Type"] == "Directory"
        size = 0 if directory else int(record.get("Size", "-1"))
        total += size
        if size < 0 or total > MAX_BYTES:
            raise ValueError("RAR exceeds the 20 GB extraction limit or has an invalid file size.")
        crc = record.get("CRC32")
        if crc is not None and not re.fullmatch(r"[0-9a-fA-F]{8}", crc):
            raise ValueError("RAR contains an invalid checksum.")
        entries.append((path, size, directory, crc))
    return entries


def _extract_rar(source, root, progress):
    with source.open("rb") as reader:
        magic = reader.read(8)
    if not (magic.startswith(b"Rar!\x1a\x07\x00") or magic == b"Rar!\x1a\x07\x01\x00"):
        raise ValueError("The file is not a supported RAR archive.")
    executable = _unrar_executable()
    with _rar_output(executable, "lt", source) as stream:
        listing = stream.read(32 * 1024 * 1024 + 1)
        if len(listing) > 32 * 1024 * 1024:
            raise ValueError("RAR listing is too large.")
    entries = _rar_entries(listing.decode("utf-8"))
    # Tek ham veri akışı kullanılır; UnRAR proje klasörüne dosya, link veya ADS yazmaz.
    with _rar_output(executable, "p", source) as stream:
        for index, (relative, size, directory, crc) in enumerate(entries):
            target = root / relative
            if directory:
                target.mkdir(parents=True, exist_ok=True)
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                remaining, checksum = size, 0
                with target.open("xb") as writer:
                    while remaining:
                        chunk = stream.read(min(1024 * 1024, remaining))
                        if not chunk:
                            raise ValueError("RAR ended before all project files were read.")
                        writer.write(chunk)
                        checksum = zlib.crc32(chunk, checksum)
                        remaining -= len(chunk)
                if crc is not None and checksum != int(crc, 16):
                    raise ValueError("RAR file checksum failed. No model was merged.")
            if progress:
                progress("Extracting RAR archive", index + 1, len(entries))
        if stream.read(1):
            raise ValueError("RAR contents do not match the archive listing.")


def prepare_source(source, project, request_id, progress=None):
    source, project = Path(source).resolve(), Path(project).resolve()
    request_id = str(uuid.UUID(request_id))
    if not project.is_dir() or not source.is_file():
        raise ValueError("Source file or project folder is unavailable.")
    if source.suffix.lower() not in MERGE_SUFFIXES:
        raise ValueError("Only MAX files and ZIP/RAR archives are supported.")
    name = re.sub(r'[^\w.-]+', "_", source.stem).strip(". ")[:32] or "model"
    root = project / "assets" / "LumiaDAM" / (name + "_" + uuid.UUID(request_id).hex[:16])
    root.mkdir(parents=True, exist_ok=False)
    try:
        if source.suffix.lower() == ".zip":
            with zipfile.ZipFile(source) as archive:
                entries = archive.infolist()
                if len(entries) > MAX_ENTRIES or sum(info.file_size for info in entries) > MAX_BYTES:
                    raise ValueError("ZIP exceeds the 100,000-file or 20 GB extraction limit.")
                paths = [_zip_path(info) for info in entries]
                if len({str(path).casefold() for path in paths}) != len(paths):
                    raise ValueError("ZIP contains duplicate Windows file paths.")
                for index, (info, relative) in enumerate(zip(entries, paths)):
                    target = root / relative
                    if info.is_dir():
                        target.mkdir(parents=True, exist_ok=True)
                    else:
                        target.parent.mkdir(parents=True, exist_ok=True)
                        with archive.open(info) as reader, target.open("xb") as writer:
                            shutil.copyfileobj(reader, writer, 1024 * 1024)
                    if progress:
                        progress("Extracting archive", index + 1, len(entries))
        elif source.suffix.lower() == ".rar":
            _extract_rar(source, root, progress)
        else:
            shutil.copy2(source, root / source.name)
        models = sorted(root.rglob("*"))
        models = [path for path in models if path.is_file() and path.suffix.lower() == ".max"]
        if not models:
            raise ValueError("The archive does not contain a MAX model.")
        return {"root": root, "models": models, "sourceRoot": source.parent,
                "files": [path for path in root.rglob("*") if path.is_file()]}
    except Exception:
        # Yalnız bu işin oluşturduğu klasörü temizleriz; mevcut proje dosyaları korunur.
        shutil.rmtree(root)
        raise


def inspect_max_candidates(runtime, models, root, byref):
    """Arşivdeki MAX sahnelerini yüklemeden okunabilirlik ve eklenti açısından inceler."""
    candidates, unreadable = [], []
    version_reader = getattr(runtime, "getMaxFileVersionData", None)
    try:
        max_version = int(runtime.maxVersion()[0]) if version_reader is not None else None
    except Exception:
        max_version = None
        version_reader = None
    for model in models:
        relative = model.relative_to(root).as_posix()
        version_label = ""
        if version_reader is not None:
            try:
                version_data = version_reader(str(model))
                source_version = int(version_data[0]) if version_data else 0
                if source_version and source_version > max_version:
                    year = 2000 + source_version // 1000 - 2
                    unreadable.append(f"{relative} (3ds Max {year})")
                    continue
                if source_version:
                    version_label = "3ds Max " + str(2000 + source_version // 1000 - 2)
            except Exception:
                pass
        try:
            names, plugins = runtime.getMAXFileObjectNames(str(model), quiet=True,
                missingDLLsAction=runtime.Name("logmsg"), missingDLLsList=byref(None))
        except Exception:
            names, plugins = None, []
        if names is None or names is False:
            unreadable.append(relative)
            continue
        folded = relative.casefold()
        renderer = "other"
        if any(token in folded for token in ("corona", "coron", "/crn/", "_crn_")):
            renderer = "corona"
        elif any(token in folded for token in ("vray", "v-ray", "v_ray", "/vr/", "_vr_")):
            renderer = "vray"
        candidates.append({"path": model, "relative": relative, "renderer": renderer, "version": version_label,
                           "objects": len(names), "plugins": list(dict.fromkeys(str(item) for item in plugins or []))})
    return candidates, unreadable


def _renderer_tag(runtime):
    try:
        name = str(runtime.classOf(runtime.renderers.current)).casefold()
    except Exception:
        return "other"
    if "corona" in name:
        return "corona"
    if "vray" in name or "v_ray" in name:
        return "vray"
    return "other"


def resolve_dependency(filename, root, files=None):
    parts = filename.replace("\\", "/").casefold().split("/")
    candidates = [path for path in (files if files is not None else Path(root).rglob("*"))
                  if path.is_file() and path.name.casefold() == parts[-1]]
    scored = []
    for candidate in candidates:
        score = 0
        for left, right in zip(reversed(parts), reversed(candidate.relative_to(root).as_posix().casefold().split("/"))):
            if left != right:
                break
            score += 1
        scored.append((score, candidate))
    if not scored:
        return None
    best = max(score for score, _ in scored)
    matches = [path for score, path in scored if score == best]
    return matches[0] if len(matches) == 1 else None


def collect_dependencies(package, filenames, progress=None):
    root = package["root"]
    paths, warnings = {}, []
    files = package["files"]
    by_name = {}
    for path in files:
        by_name.setdefault(path.name.casefold(), []).append(path)
    filenames = list(dict.fromkeys(filenames))
    for index, filename in enumerate(filenames):
        local = resolve_dependency(filename, root, by_name.get(ntpath.basename(filename).casefold(), []))
        original = Path(filename)
        if not original.is_absolute():
            original = package["sourceRoot"] / original
        # Erişilebilir eski NAS yolu da proje içine kopyalanır; bağımlılık NAS'ta kalmaz.
        if local is None and original.is_file():
            parent_id = hashlib.sha256(str(original.parent.resolve()).casefold().encode("utf-8")).hexdigest()[:16]
            local = root / "external" / parent_id / original.name
            local.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(original, local)
        if local is None:
            warnings.append("Missing or ambiguous dependency: " + filename)
        else:
            paths[filename] = str(local)
        if progress:
            progress("Collecting dependencies", index + 1, len(filenames))
    return {"paths": paths, "warnings": warnings}


def _write_json(path, value):
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(value, ensure_ascii=False), encoding="utf-8")
    os.replace(temporary, path)


def required_plugins(missing):
    # Eski görünüm yöneticisi ve mental ray özel nitelikleri materyal/ışık sınıfı değildir.
    return [str(plugin) for plugin in missing or []
            if str(plugin).split(" - ", 1)[0].casefold() not in ("viewportmanager.gup", "mrmaterialattribs.gup")]


def relink_live_files(runtime, nodes, package, paths, warnings):
    """Yalnız içe alınan nesnelerin canlı bitmap ve IES slotlarını proje kopyalarına bağlar."""
    normalized = {ntpath.normcase(key): value for key, value in paths.items()}
    by_name = {}
    for path in package.get("files", []):
        by_name.setdefault(path.name.casefold(), []).append(path)
    classes = [getattr(runtime, name, None) for name in ("BitmapTexture", "CoronaBitmap", "VRayHDRI", "VRayBitmap")]
    seen, verified, changed = set(), 0, 0
    root = package["root"].resolve()
    for node in nodes:
        references = [node]
        for cls in classes:
            if cls is not None:
                references.extend(runtime.getClassInstances(cls, target=node, processChildren=False))
        for reference in references:
            if reference is None:
                continue
            try:
                key = int(runtime.getHandleByAnim(reference))
                if key in seen:
                    continue
                seen.add(key)
                properties = runtime.getPropNames(reference)
            except (AttributeError, RuntimeError):
                continue
            for prop in properties:
                if str(prop).lower().lstrip("#") not in ("filename", "hdri_mapname", "hdrimapname", "iesfile", "ies_file", "webfile"):
                    continue
                value = runtime.getProperty(reference, prop)
                if not isinstance(value, str) or not value:
                    continue
                target = normalized.get(ntpath.normcase(value))
                if target is None:
                    target = resolve_dependency(value, root, by_name.get(ntpath.basename(value).casefold(), []))
                if target is None or not Path(target).resolve().is_relative_to(root) or not Path(target).is_file():
                    warnings.append("Live file reference is missing or ambiguous: " + value)
                    continue
                try:
                    target = str(target)
                    if ntpath.normcase(value) != ntpath.normcase(target):
                        runtime.setProperty(reference, prop, target)
                        changed += 1
                    if ntpath.normcase(str(runtime.getProperty(reference, prop))) != ntpath.normcase(target):
                        raise ValueError("3ds Max did not accept the new file path")
                    verified += 1
                except Exception as error:
                    warnings.append(f"Could not relink {value}: {error}")
    return {"verified": verified, "changed": changed}


def load_corona_converter(runtime):
    """Kullanıcının kurulu Corona dönüştürücüsünü bulur; üçüncü taraf betik dağıtılmaz."""
    converter = getattr(runtime, "CoronaConverter", None)
    if converter is not None:
        return converter
    year = int(runtime.maxVersion()[0]) // 1000 + 1998
    scripts = [Path(str(runtime.getDir(runtime.Name("maxRoot")))) / "scripts/CoronaRenderer/coronaConverter.ms"]
    if os.environ.get("ProgramFiles"):
        scripts.append(Path(os.environ["ProgramFiles"]) / f"Chaos/Corona/Corona Renderer for 3ds Max/{year}/Scripts/coronaConverter.ms")
    manager = runtime.pluginManager
    for index in range(1, int(manager.pluginDllCount) + 1):
        if "corona" not in str(manager.pluginDllName(index)).casefold():
            continue
        directory = Path(str(manager.pluginDllFullPath(index))).resolve().parent
        scripts.extend((directory / "Scripts/coronaConverter.ms", directory.parent / "Scripts/coronaConverter.ms"))
    for script in dict.fromkeys(scripts):
        if not script.is_file():
            continue
        previous = getattr(runtime, "CoronaConverterSuppressGui", None)
        runtime.CoronaConverterSuppressGui = True
        try:
            runtime.fileIn(str(script))
        finally:
            runtime.CoronaConverterSuppressGui = previous
        converter = getattr(runtime, "CoronaConverter", None)
        if converter is not None:
            return converter
    raise ValueError("The installed Corona Converter could not be found.")


def material_classes(runtime, nodes):
    classes, seen = {}, set()
    pending = [getattr(node, "material", None) for node in nodes]
    while pending:
        material = pending.pop()
        if material is None:
            continue
        key = int(runtime.getHandleByAnim(material))
        if key in seen:
            continue
        seen.add(key)
        name = str(runtime.classOf(material))
        classes[name] = classes.get(name, 0) + 1
        pending.extend(runtime.getSubMtl(material, index) for index in range(1, runtime.getNumSubMtls(material) + 1))
    return classes


def convert_imported_to_corona(runtime, nodes):
    """Dönüşüm yalnız yeni nesnelere uygulanır; mevcut sahnenin render ayarları korunur."""
    converter = load_corona_converter(runtime)
    settings = converter.converterSettings
    saved = {name: getattr(settings, name) for name in ("switchRenderEngine", "showWarnings", "printInfo")}
    try:
        settings.switchRenderEngine = settings.showWarnings = settings.printInfo = False
        # Mevcut sahneyle paylaşılan materyal bulunursa global replaceInstances onu değiştirmesin.
        materials = {}
        for node in nodes:
            material = getattr(node, "material", None)
            if material is not None:
                key = int(runtime.getHandleByAnim(material))
                if key not in materials:
                    materials[key] = runtime.copy(material)
                node.material = materials[key]
        converter.convertNodeArray(runtime.Array(*nodes), ErrorReport=False)
        lights = 0
        for node in nodes:
            if runtime.superClassOf(node) != runtime.Light:
                continue
            replacement = converter.createCoronaLights(node)
            if str(replacement) == "NoReplacement" or replacement == node:
                continue
            if runtime.isValidNode(replacement):
                runtime.replaceInstances(node, replacement)
                runtime.delete(replacement)
                lights += 1
        converter.cleanConversionData()
        return {"materials": sum(count for name, count in material_classes(runtime, nodes).items() if name.lower().lstrip("_").startswith("corona")), "lights": lights}
    finally:
        for name, value in saved.items():
            setattr(settings, name, value)


class Bridge:
    def __init__(self, runtime, qtcore, widgets):
        self.rt, self.widgets = runtime, widgets
        self.instance_id = str(uuid.uuid4())
        self.root = Path(os.environ["LOCALAPPDATA"]) / "LumiaDAM/bridge/3dsmax" / self.instance_id
        (self.root / "requests").mkdir(parents=True, exist_ok=True)
        (self.root / "responses").mkdir(exist_ok=True)
        self.executor = ThreadPoolExecutor(max_workers=1)
        self.current = None
        self.ticking = False
        self.last_progress = 0
        self.timer = qtcore.QTimer()
        self.timer.timeout.connect(self.tick)
        self.timer.start(1000)
        self.tick()

    def scene_path(self):
        return str(self.rt.maxFilePath) + str(self.rt.maxFileName)

    def respond(self, state, phase, **extra):
        _write_json(self.root / "responses" / (self.current["request"]["id"] + ".json"),
                    dict(id=self.current["request"]["id"], state=state, phase=phase, **extra))

    def progress(self, phase, completed, total):
        if completed == total or time.monotonic() - self.last_progress > 0.1:
            self.last_progress = time.monotonic()
            self.respond("running", phase, completed=completed, total=total)

    def capture_metadata(self, models):
        if len(self.current["metadata"]) + len(models) > 64:
            raise ValueError("Too many nested MAX references (limit: 64).")
        refs = []
        for model in models:
            metadata = self.rt.getMAXFileAssetMetadata(str(model))
            self.current["metadata"][str(model)] = metadata
            for asset in metadata:
                if str(asset.type).lower().lstrip("#") in ("script", "prerenderscript", "postrenderscript", "renderoutput"):
                    continue
                filename = str(asset.filename)
                if filename:
                    refs.append(filename)
        self.current["stage"] = "collect"
        self.current["future"] = self.executor.submit(collect_dependencies, self.current["package"], refs, self.progress)

    def finish_merge(self):
        from pymxs import byref
        job = self.current
        if ntpath.normcase(self.scene_path()) != ntpath.normcase(job["request"]["scenePath"]):
            raise ValueError("The target scene changed. Merge was not started.")
        self.respond("running", "Checking model material and light compatibility")
        missing = []
        # Nesne adları ve eksik sınıflar sahneye merge edilmeden okunabilir.
        for model in job["metadata"] or [str(job["model"])]:
            names, plugins = self.rt.getMAXFileObjectNames(model, quiet=True,
                missingDLLsAction=self.rt.Name("logmsg"), missingDLLsList=byref(None))
            if names is None or names is False:
                raise ValueError("The MAX model could not be inspected. Nothing was merged.")
            missing.extend(str(plugin) for plugin in plugins or [])
        missing = list(dict.fromkeys(missing))
        critical = required_plugins(missing)
        job["warnings"].extend(missing)
        # Native Merge gibi eksik eklenti bilgisi aktarımı engellemez; dönüşüm kararı canlı nesnelerle verilir.
        job["incompleteMerge"] = bool(critical)
        self.respond("running", "Updating project file references")
        for filename, metadata in job["metadata"].items():
            for asset in metadata:
                target = job["paths"].get(str(asset.filename))
                if target:
                    asset.filename = target
            self.rt.setMAXFileAssetMetadata(filename, metadata)
        self.respond("running", "Merging into the active 3ds Max scene")
        # Yalnız gelen nesne ve materyaller yeniden adlandırılır; sahne otomatik kaydedilmez.
        result, missing_plugins = self.rt.mergeMAXFile(str(job["model"]), self.rt.Name("select"),
                                      self.rt.Name("autoRenameDups"), self.rt.Name("renameMtlDups"),
                                      quiet=True, missingDLLsAction=self.rt.Name("logmsg"),
                                      missingDLLsList=byref(None),
                                      missingExtFilesAction=self.rt.Name("logmsg"),
                                      missingXRefsAction=self.rt.Name("logmsg"))
        # Eski render bileşenleri geometriyi de engellemesin; eksik materyaller gizlenmez.
        if missing_plugins:
            job["warnings"].append("Some materials or objects may be incomplete because these 3ds Max plugins are unavailable:")
            job["warnings"].extend(str(plugin) for plugin in missing_plugins)
            if required_plugins(missing_plugins):
                job["incompleteMerge"] = True
        if not result:
            raise ValueError("3ds Max could not merge this model. Check its Max version and required renderer/plugins.")
        nodes = list(self.rt.getLastMergedNodes())
        self.respond("running", "Relinking live bitmap and light file references")
        linked = relink_live_files(self.rt, nodes, job["package"], job["paths"], job["warnings"])
        classes = material_classes(self.rt, nodes)
        source_vray = any("vray" in value.casefold() or "vrender" in value.casefold() for value in missing + list(classes))
        source_vray = source_vray or any("vray" in str(self.rt.classOf(node)).casefold() for node in nodes)
        conversion = None
        if source_vray and getattr(self.rt, "Corona", None) is not None:
            self.respond("running", "Waiting for Corona conversion choice in 3ds Max")
            box = self.widgets.QMessageBox
            language = _language()
            text = _t("This model uses V-Ray materials or lights. Convert the imported objects to Corona?\n\n"
                    "Yes: use the installed Corona Converter for imported objects only.\n"
                    "No: keep the source materials and lights, like a normal Max merge.\n\n"
                    "The current renderer, environment, and existing scene objects will be preserved.", language)
            if critical:
                text += _t("\n\nSome source plugin definitions are missing. Conversion will be checked; unreadable source materials cannot be reconstructed automatically.", language)
            dialog = box(box.Question, _t("LumiaDAM — Convert V-Ray to Corona?", language), text, box.Yes | box.No)
            dialog.button(box.Yes).setText(_t("Yes", language))
            dialog.button(box.No).setText(_t("No", language))
            dialog.setDefaultButton(box.No)
            if (getattr(dialog, "exec", None) or dialog.exec_)() == box.Yes:
                self.respond("running", "Converting imported materials and lights to Corona")
                try:
                    conversion = convert_imported_to_corona(self.rt, nodes)
                    after_linked = relink_live_files(self.rt, nodes, job["package"], job["paths"], job["warnings"])
                    linked = {"verified": after_linked["verified"], "changed": linked["changed"] + after_linked["changed"]}
                    if not conversion["materials"] and not conversion["lights"]:
                        job["warnings"].append("Corona Converter did not produce any Corona materials or lights. The source plugin definitions must be readable; conversion was not successful.")
                except Exception as error:
                    job["warnings"].append("Corona conversion could not be completed: " + str(error))
                    job["incompleteMerge"] = True
        if critical:
            job["warnings"].append("Some source materials, maps, or lights could not be read because their plugin definitions are unavailable. The objects were kept; inspect before rendering.")
        self.rt.redrawViews()
        warnings = list(dict.fromkeys(job["warnings"]))
        if len(warnings) > 200:
            warnings = warnings[:200] + ["Additional dependency warnings: " + str(len(warnings) - 200)]
        incomplete = job.get("incompleteMerge", False)
        phase = "Incomplete import — materials, lights, or plugin objects unavailable" if incomplete else "Merge complete with warnings" if warnings else "Merge complete"
        self.respond("complete", phase, importedNodes=len(nodes), incompleteMerge=incomplete,
                     liveFileReferences=linked["verified"], relinkedFiles=linked["changed"], coronaConversion=conversion,
                     destinationPath=str(job["package"]["root"]),
                     copiedFiles=sum(1 for p in job["package"]["root"].rglob("*") if p.is_file()),
                     warnings=warnings)
        self.current = None

    def tick(self):
        if self.ticking:
            return
        self.ticking = True
        try:
            _write_json(self.root / "heartbeat.json", dict(id=self.instance_id, version=PROTOCOL_VERSION,
                        mergeExtensions=[suffix[1:] for suffix in MERGE_SUFFIXES],
                        pid=os.getpid(), maxVersion=str(self.rt.maxVersion()[0]), scenePath=self.scene_path(),
                        projectPath=str(self.rt.pathConfig.getCurrentProjectFolder()),
                        busy=self.current is not None, updatedAt=time.time()))
            if self.current is None:
                for path in sorted((self.root / "requests").glob("*.json")):
                    try:
                        if path.stat().st_size > 65536:
                            raise ValueError("Bridge request is too large.")
                        request = json.loads(path.read_text(encoding="utf-8"))
                        if path.stem != str(uuid.UUID(request["id"])):
                            raise ValueError("Bridge request identifier mismatch.")
                    except (ValueError, KeyError, TypeError):
                        path.unlink()
                        continue
                    self.current = {"request": request}
                    path.unlink()
                    validate_request(request)
                    if ntpath.normcase(self.scene_path()) != ntpath.normcase(request["scenePath"]):
                        raise ValueError("The target scene changed. Select the Max scene again.")
                    self.current.update(stage="prepare", metadata={}, paths={}, warnings=[])
                    self.respond("running", "Preparing project files")
                    self.current["future"] = self.executor.submit(prepare_source, request["sourcePath"],
                        request["projectPath"], request["id"], self.progress)
                    break
            elif self.current["future"].done():
                job = self.current
                result = job["future"].result()
                if job["stage"] == "prepare":
                    job["package"] = result
                    models = result["models"]
                    from pymxs import byref
                    candidates, unreadable = inspect_max_candidates(self.rt, models, result["root"], byref)
                    language = _language()
                    if not candidates:
                        raise ValueError(_t("No readable MAX models were found in the archive.", language) +
                            (" " + ", ".join(unreadable) if unreadable else ""))
                    if len(candidates) > 1:
                        self.respond("running", "Choose a MAX model in the 3ds Max window")
                        labels = []
                        current_renderer = _renderer_tag(self.rt)
                        for candidate in candidates:
                            renderer_label = candidate["renderer"].upper() if candidate["renderer"] != "other" else _t("Unknown renderer", language)
                            object_word = "object" if candidate["objects"] == 1 else "objects"
                            plugin_word = "missing plugin" if len(candidate["plugins"]) == 1 else "missing plugins"
                            detail = f"{candidate['objects']} {_t(object_word, language)} · {len(candidate['plugins'])} {_t(plugin_word, language)}"
                            if candidate["version"]:
                                detail += " · " + candidate["version"]
                            if candidate["renderer"] == current_renderer and current_renderer != "other":
                                detail += " · " + _t("current renderer", language)
                            labels.append(f"{candidate['relative']} · {renderer_label} · {detail}")
                        dialog = self.widgets.QInputDialog()
                        dialog.setWindowTitle(_t("LumiaDAM — Select model", language))
                        prompt = _t("Choose the MAX file to merge:", language)
                        if unreadable:
                            prompt += "\n" + _t("Incompatible or unreadable MAX files were skipped:", language) + " " + ", ".join(unreadable)
                        dialog.setLabelText(prompt)
                        dialog.setComboBoxItems(labels)
                        dialog.setComboBoxEditable(False)
                        dialog.setOkButtonText(_t("OK", language))
                        dialog.setCancelButtonText(_t("Cancel", language))
                        matching = [index for index, item in enumerate(candidates)
                                    if current_renderer != "other" and item["renderer"] == current_renderer]
                        if len(matching) == 1:
                            dialog.setTextValue(labels[matching[0]])
                        accepted = (getattr(dialog, "exec", None) or dialog.exec_)()
                        if not accepted:
                            raise ValueError("Model selection cancelled. Project copies have been kept.")
                        model = candidates[labels.index(dialog.textValue())]["path"]
                    else:
                        model = candidates[0]["path"]
                    job["model"] = model
                    self.capture_metadata([model])
                else:
                    job["paths"].update(result["paths"])
                    job["warnings"].extend(result["warnings"])
                    nested = [Path(p) for p in result["paths"].values() if Path(p).suffix.lower() == ".max" and p not in job["metadata"]]
                    if nested:
                        self.capture_metadata(list(dict.fromkeys(nested)))
                    else:
                        self.finish_merge()
        except Exception as error:
            if self.current:
                destination = str(self.current.get("package", {}).get("root", ""))
                self.respond("error", str(error), destinationPath=destination,
                             warnings=list(dict.fromkeys(self.current.get("warnings", [])))[:200])
                self.current = None
            else:
                print("LumiaDAM Bridge: " + str(error))
        finally:
            self.ticking = False


def start():
    from pymxs import runtime
    try:
        from PySide6 import QtCore, QtWidgets
    except ImportError:
        from PySide2 import QtCore, QtWidgets
    import builtins
    if not hasattr(builtins, "_lumiadam_bridge"):
        builtins._lumiadam_bridge = Bridge(runtime, QtCore, QtWidgets)


if __name__ == "__main__":
    start()
