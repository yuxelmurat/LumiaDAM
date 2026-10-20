"""Max dışındaki aktarım davranışlarını gerçek dosyalarla doğrular."""
import importlib.util
import json
import os
import subprocess
import tempfile
import unittest
import uuid
import zipfile
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

MODULE_PATH = Path(__file__).parent / "LumiaDAM.bundle/Contents/lumiadam_max.py"
bridge = None
if MODULE_PATH.exists():
    spec = importlib.util.spec_from_file_location("lumiadam_max", MODULE_PATH)
    bridge = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(bridge)


class MergeTests(unittest.TestCase):
    def setUp(self):
        self.assertIsNotNone(bridge, "LumiaDAM Max aktarımı henüz uygulanmadı")
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.project = self.root / "project"
        self.project.mkdir()

    def archive(self, files):
        path = self.root / "chair.zip"
        with zipfile.ZipFile(path, "w") as archive:
            for name, content in files.items():
                archive.writestr(name, content)
        return path

    def prepare(self, path):
        return bridge.prepare_source(path, self.project, str(uuid.uuid4()))

    def rar_archive(self, files, password=None, extra=()):
        executable = Path(os.environ.get("ProgramFiles", "C:/Program Files")) / "WinRAR/Rar.exe"
        if not executable.is_file():
            self.skipTest("Gerçek RAR üretimi için mevcut WinRAR gerekiyor")
        directory = self.root / "rar-source"
        directory.mkdir(exist_ok=True)
        for name, content in files.items():
            target = directory / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(content)
        source = self.root / "İç mimari örnek.RAR"
        subprocess.run([str(executable), "a", "-idq", "-ep1", "-r", *extra, *( ["-p" + password] if password else []),
                        str(source), "*"], cwd=directory, stdin=subprocess.DEVNULL, capture_output=True, check=True, timeout=30)
        return source

    def test_rar_contents_and_unicode_paths_stay_together_in_project(self):
        source = self.rar_archive({"corona/ünite.max": b"model", "maps/ahşap.jpg": b"texture", "reference/render.jpg": b"render"})
        original = source.read_bytes()
        updates = []
        package = bridge.prepare_source(source, self.project, str(uuid.uuid4()), lambda *args: updates.append(args))
        self.assertEqual(package["models"][0].read_bytes(), b"model")
        self.assertEqual((package["root"] / "maps/ahşap.jpg").read_bytes(), b"texture")
        self.assertEqual((package["root"] / "reference/render.jpg").read_bytes(), b"render")
        self.assertEqual(source.read_bytes(), original)
        self.assertTrue(updates)

    def test_rar_with_multiple_max_files_keeps_both_renderer_variants(self):
        source = self.rar_archive({"corona/chair.max": b"c", "vray/chair.max": b"v"})
        self.assertEqual(len(self.prepare(source)["models"]), 2)

    def test_rar_without_max_and_encrypted_rar_do_not_leave_project_copies(self):
        source = self.rar_archive({"render.jpg": b"render"})
        with self.assertRaisesRegex(ValueError, "MAX"):
            self.prepare(source)
        source.unlink()
        source = self.rar_archive({"chair.max": b"model"}, password="test-only")
        with self.assertRaisesRegex(ValueError, "Password-protected|encrypted"):
            self.prepare(source)
        self.assertFalse(any((self.project / "assets/LumiaDAM").glob("*")))

    def test_corrupt_rar_checksum_cleans_only_its_project_folder(self):
        payload = b"lumiadam-test-unique-model-data"
        source = self.rar_archive({"chair.max": payload}, extra=("-m0",))
        data = bytearray(source.read_bytes())
        data[data.index(payload)] ^= 1
        source.write_bytes(data)
        existing = self.project / "keep.txt"
        existing.write_bytes(b"existing project")
        with self.assertRaisesRegex(ValueError, "checksum"):
            self.prepare(source)
        self.assertEqual(existing.read_bytes(), b"existing project")
        self.assertFalse(any((self.project / "assets/LumiaDAM").glob("*")))

    def test_rar_size_limit_is_checked_before_copying(self):
        source = self.rar_archive({"chair.max": b"123456"})
        original = bridge.MAX_BYTES
        bridge.MAX_BYTES = 4
        try:
            with self.assertRaisesRegex(ValueError, "extraction limit"):
                self.prepare(source)
        finally:
            bridge.MAX_BYTES = original
        self.assertFalse(any((self.project / "assets/LumiaDAM").glob("*")))

    def test_rar_metadata_rejects_traversal_links_and_duplicate_windows_paths(self):
        valid = "        Name: chair.max\n        Type: File\n        Size: 1\n\n"
        for entry in ["Name: ../escape.jpg\nType: File\nSize: 0", "Name: link\nType: Windows symbolic link\nTarget: ../outside",
                      "Name: CHAIR.MAX\nType: File\nSize: 0", "Name: bad.max\ncontinued.jpg\nType: File\nSize: 0",
                      "Name: bad.max\nType: File\nSize: 0\nSize: 1"]:
            with self.subTest(entry=entry), self.assertRaises(ValueError):
                bridge._rar_entries(valid + entry)

    def test_rar_bridge_request_is_supported(self):
        import time
        bridge.validate_request({"version": 1, "action": "merge", "id": str(uuid.uuid4()),
                                 "sourcePath": "A:/models/Chair.RAR", "createdAt": time.time()})

    def test_zip_contents_stay_together_in_project(self):
        source = self.archive({"corona/chair.max": b"model", "maps/wood.jpg": b"texture", "reference/render.jpg": b"render"})
        package = self.prepare(source)
        self.assertEqual(package["models"][0].read_bytes(), b"model")
        self.assertEqual((package["root"] / "maps/wood.jpg").read_bytes(), b"texture")
        self.assertEqual((package["root"] / "reference/render.jpg").read_bytes(), b"render")
        self.assertTrue(package["root"].is_relative_to(self.project))
        self.assertTrue(source.exists())

    def test_repeated_imports_do_not_overwrite_project_assets(self):
        source = self.archive({"chair.max": b"model"})
        first, second = self.prepare(source), self.prepare(source)
        self.assertNotEqual(first["root"], second["root"])
        self.assertEqual(first["models"][0].read_bytes(), b"model")

    def test_unsafe_archive_paths_are_rejected_before_extraction(self):
        for name in ["../escape.jpg", "C:/escape.jpg", "/escape.jpg", "maps/a.jpg:stream", "maps/CON.jpg", "maps/../escape.jpg"]:
            with self.subTest(name=name):
                source = self.archive({"chair.max": b"model", name: b"bad"})
                with self.assertRaises(ValueError):
                    self.prepare(source)
        self.assertFalse((self.root / "escape.jpg").exists())

    def test_case_insensitive_duplicate_archive_names_are_rejected(self):
        source = self.archive({"chair.max": b"model", "Maps/wood.jpg": b"a", "maps/WOOD.jpg": b"b"})
        with self.assertRaises(ValueError):
            self.prepare(source)

    def test_archive_without_max_is_not_reported_as_success(self):
        source = self.archive({"render.jpg": b"render"})
        with self.assertRaisesRegex(ValueError, "MAX"):
            self.prepare(source)

    def test_multiple_models_are_kept_for_explicit_selection(self):
        source = self.archive({"corona/chair.max": b"c", "vray/chair.max": b"v"})
        self.assertEqual(len(self.prepare(source)["models"]), 2)

    def test_matching_dependency_uses_folder_suffix(self):
        package = self.prepare(self.archive({"chair.max": b"m", "oak/wood.jpg": b"o", "pine/wood.jpg": b"p"}))
        match = bridge.resolve_dependency("Z:/old/pine/wood.jpg", package["root"])
        self.assertEqual(match, package["root"] / "pine/wood.jpg")

    def test_ambiguous_dependency_is_not_guessed(self):
        package = self.prepare(self.archive({"chair.max": b"m", "oak/wood.jpg": b"o", "pine/wood.jpg": b"p"}))
        self.assertIsNone(bridge.resolve_dependency("Z:/old/wood.jpg", package["root"]))

    def test_valid_source_dependency_is_copied_and_relinked_locally(self):
        model = self.root / "chair.max"
        model.write_bytes(b"model")
        texture = self.root / "textures/wood.jpg"
        texture.parent.mkdir()
        texture.write_bytes(b"texture")
        package = self.prepare(model)
        result = bridge.collect_dependencies(package, [str(texture)])
        local = Path(result["paths"][str(texture)])
        self.assertTrue(local.is_relative_to(package["root"]))
        self.assertEqual(local.read_bytes(), b"texture")
        self.assertTrue(texture.exists())
        self.assertEqual(model.read_bytes(), b"model")

    def test_missing_and_ambiguous_dependencies_are_reported(self):
        package = self.prepare(self.archive({"chair.max": b"m", "a/wood.jpg": b"a", "b/wood.jpg": b"b"}))
        result = bridge.collect_dependencies(package, ["Z:/missing.jpg", "Z:/wood.jpg"])
        self.assertEqual(result["paths"], {})
        self.assertEqual(len(result["warnings"]), 2)

    def test_large_archive_limit_is_enforced_before_files_are_written(self):
        source = self.archive({"chair.max": b"123456"})
        original = bridge.MAX_BYTES
        bridge.MAX_BYTES = 4
        try:
            with self.assertRaises(ValueError):
                self.prepare(source)
        finally:
            bridge.MAX_BYTES = original

    def test_unknown_bridge_action_is_rejected(self):
        with self.assertRaises(ValueError):
            bridge.validate_request({"version": 1, "action": "execute", "id": str(uuid.uuid4())})

    def finish_bridge(self, successful=True, allow_incomplete=True, missing=None):
        calls, responses = [], []
        package = self.prepare(self.archive({"chair.max": b"model"}))
        missing = ["mentalray_plugins.dlo - Arch & Design", "iraymaterial.dlt - iray Material"] if missing is None else missing

        def merge(*args, **kwargs):
            calls.append((args, kwargs))
            # Eski ayar, kullanılmayan render bileşenleri için dahi bütün aktarımı iptal eder.
            if kwargs["missingDLLsAction"] == "abort":
                return False
            return successful, missing

        worker = bridge.Bridge.__new__(bridge.Bridge)
        worker.rt = SimpleNamespace(Name=lambda value: value, mergeMAXFile=merge,
                                    getLastMergedNodes=lambda: [object()], redrawViews=lambda: None,
                                    getMAXFileObjectNames=lambda *args, **kwargs: (["chair"], missing),
                                    classOf=lambda value: "Corona", renderers=SimpleNamespace(current=object()))
        dialogs = []

        class MessageBox:
            Yes, No, Question = 1, 2, 4

            def __init__(self, icon, title, text, buttons):
                self.title, self.text, self.buttons = title, text, {}
                dialogs.append(self)

            def button(self, button):
                return SimpleNamespace(setText=lambda label: self.buttons.__setitem__(button, label))

            def setDefaultButton(self, button):
                self.default = button

            def exec(self):
                return self.Yes if allow_incomplete else self.No

        worker.widgets = SimpleNamespace(QMessageBox=MessageBox)
        worker.dialogs = dialogs
        worker.current = {"request": {"scenePath": "main.max"}, "metadata": {}, "paths": {}, "warnings": [],
                          "package": package, "model": package["models"][0]}
        worker.scene_path = lambda: "main.max"
        worker.respond = lambda state, phase, **extra: responses.append(dict(state=state, phase=phase, **extra))
        return worker, calls, responses

    def test_language_defaults_to_english_and_reads_changes_without_restart(self):
        with patch.dict(os.environ, {"LOCALAPPDATA": str(self.root)}):
            self.assertEqual(bridge._language(), "en")
            preferences = self.root / "LumiaDAM/preferences.json"
            preferences.parent.mkdir()
            for value in ['{}', '{"language":"de"}', '[]', '{broken', '"tr"', 'null']:
                preferences.write_text(value, encoding="utf-8")
                self.assertEqual(bridge._language(), "en", value)
            for language in ("tr", "en", "tr"):
                preferences.write_text(json.dumps({"language": language}), encoding="utf-8-sig")
                self.assertEqual(bridge._language(), language)
                self.assertEqual(bridge._t("Cancel", bridge._language()), "İptal" if language == "tr" else "Cancel")
            self.assertEqual(bridge._t("C:/Kitchen/Office.max", "tr"), "C:/Kitchen/Office.max")

    def test_conversion_dialog_uses_saved_language_and_keeps_english_bridge_protocol(self):
        preferences = self.root / "LumiaDAM/preferences.json"
        preferences.parent.mkdir()
        for language, caption, title in [("en", "Yes", "Convert V-Ray to Corona?"), ("tr", "Evet", "Corona biçimine dönüştürülsün mü?")]:
            preferences.write_text(json.dumps({"language": language}), encoding="utf-8")
            worker, calls, responses = self.finish_bridge(allow_incomplete=False, missing=["vrender2015.dlr - VRayMtl"])
            worker.rt.Corona = object()
            with patch.dict(os.environ, {"LOCALAPPDATA": str(self.root)}), patch.dict(
                "sys.modules", {"pymxs": SimpleNamespace(byref=lambda value: value)}):
                worker.finish_merge()
            dialog = worker.dialogs[0]
            self.assertIn(title, dialog.title)
            self.assertEqual(dialog.buttons[dialog.Yes], caption)
            self.assertEqual(dialog.buttons[dialog.No], "Hayır" if language == "tr" else "No")
            self.assertEqual(dialog.default, dialog.No)
            self.assertIn("otomatik olarak yeniden oluşturulamaz" if language == "tr" else "cannot be reconstructed automatically", dialog.text)
            self.assertIn("Mevcut render motoru" if language == "tr" else "The current renderer", dialog.text)
            self.assertEqual(len(calls), 1)
            self.assertEqual(responses[-1]["state"], "complete")
            self.assertEqual(responses[-1]["phase"], "Incomplete import — materials, lights, or plugin objects unavailable")
            self.assertIsNone(responses[-1]["coronaConversion"])

    def test_model_choice_translates_controls_and_preserves_selected_filename(self):
        for language, accepted in [("en", True), ("tr", True), ("tr", False)]:
            worker, _, responses = self.finish_bridge()
            package = self.prepare(self.archive({"Kitchen.max": b"first", "Office.max": b"second"}))
            dialogs, selected = [], []

            class InputDialog:
                def __init__(self):
                    self.values = {}
                    dialogs.append(self)

                def __getattr__(self, name):
                    return lambda value: self.values.__setitem__(name, value)

                def textValue(self):
                    return self.values["setComboBoxItems"][1]

                def exec(self):
                    return int(accepted)

            worker.widgets.QInputDialog = InputDialog
            worker.rt.maxVersion = lambda: [26000]
            worker.rt.pathConfig = SimpleNamespace(getCurrentProjectFolder=lambda: str(self.project))
            worker.root, worker.instance_id, worker.ticking = self.root, "test-instance", False
            worker.current.update(stage="prepare", future=SimpleNamespace(done=lambda: True, result=lambda: package))
            worker.capture_metadata = lambda models: selected.extend(models)
            with patch.object(bridge, "_language", return_value=language), patch.object(bridge, "_write_json"), patch.dict(
                "sys.modules", {"pymxs": SimpleNamespace(byref=lambda value: value)}):
                worker.tick()
            values = dialogs[0].values
            self.assertEqual(values["setWindowTitle"], "LumiaDAM — Model seç" if language == "tr" else "LumiaDAM — Select model")
            self.assertIn("Birleştirilecek MAX dosyasını seçin:" if language == "tr" else "Choose the MAX file to merge:", values["setLabelText"])
            self.assertEqual(values["setOkButtonText"], "Tamam" if language == "tr" else "OK")
            self.assertEqual(values["setCancelButtonText"], "İptal" if language == "tr" else "Cancel")
            self.assertIn("Kitchen.max", values["setComboBoxItems"][0])
            self.assertIn("Office.max", values["setComboBoxItems"][1])
            self.assertIn("1 nesne" if language == "tr" else "1 object", values["setComboBoxItems"][0])
            self.assertEqual(selected, [package["models"][1]] if accepted else [])
            self.assertTrue(all(model.exists() for model in package["models"]))
            if not accepted:
                self.assertEqual(responses[-1]["state"], "error")
                self.assertEqual(responses[-1]["phase"], "Model selection cancelled. Project copies have been kept.")

    def test_candidate_inspection_skips_unreadable_models_and_reports_renderer_and_plugin_details(self):
        package = self.prepare(self.archive({"corona/chair.max": b"c", "vray/chair.max": b"v", "bad/scene.max": b"x"}))

        def inspect(path, **kwargs):
            path = path.replace("\\", "/")
            if path.endswith("bad/scene.max"):
                return None, []
            if "/corona/" in path.replace("\\", "/"):
                return ["Chair", "Table"], ["CoronaLegacy.dll"]
            return ["Desk"], []

        runtime = SimpleNamespace(getMAXFileObjectNames=inspect, Name=lambda value: value)
        candidates, unreadable = bridge.inspect_max_candidates(runtime, package["models"], package["root"], lambda value: value)
        self.assertEqual([candidate["renderer"] for candidate in candidates], ["corona", "vray"])
        self.assertEqual([candidate["objects"] for candidate in candidates], [2, 1])
        self.assertEqual(candidates[0]["plugins"], ["CoronaLegacy.dll"])
        self.assertEqual(unreadable, ["bad/scene.max"])

    def test_candidate_inspection_skips_files_saved_by_a_newer_max_version(self):
        package = self.prepare(self.archive({"old/chair.max": b"old", "new/chair.max": b"new"}))
        runtime = SimpleNamespace(maxVersion=lambda: [26000], Name=lambda value: value,
            getMaxFileVersionData=lambda path: [27000] if "new" in path else [26000],
            getMAXFileObjectNames=lambda *args, **kwargs: (["Chair"], []))
        candidates, unreadable = bridge.inspect_max_candidates(runtime, package["models"], package["root"], lambda value: value)
        self.assertEqual([candidate["relative"] for candidate in candidates], ["old/chair.max"])
        self.assertEqual(candidates[0]["version"], "3ds Max 2024")
        self.assertEqual(unreadable, ["new/chair.max (3ds Max 2025)"])

    def test_model_choice_defaults_to_the_only_candidate_matching_current_renderer(self):
        worker, _, _ = self.finish_bridge()
        package = self.prepare(self.archive({"vray/chair.max": b"v", "corona/chair.max": b"c"}))
        dialogs, selected = [], []

        class InputDialog:
            def __init__(self):
                self.values = {}
                dialogs.append(self)

            def __getattr__(self, name):
                return lambda value: self.values.__setitem__(name, value)

            def textValue(self):
                return self.values["setTextValue"]

            def exec(self):
                return 1

        worker.widgets.QInputDialog = InputDialog
        worker.rt.classOf = lambda value: "CoronaRenderer"
        worker.rt.maxVersion = lambda: [26000]
        worker.rt.pathConfig = SimpleNamespace(getCurrentProjectFolder=lambda: str(self.project))
        worker.root, worker.instance_id, worker.ticking = self.root, "test-instance", False
        worker.current.update(stage="prepare", future=SimpleNamespace(done=lambda: True, result=lambda: package))
        worker.capture_metadata = lambda models: selected.extend(models)
        with patch.object(bridge, "_language", return_value="en"), patch.object(bridge, "_write_json"), patch.dict(
            "sys.modules", {"pymxs": SimpleNamespace(byref=lambda value: value)}):
            worker.tick()
        selected_label = dialogs[0].values["setTextValue"]
        self.assertIn("corona/chair.max", selected_label)
        self.assertIn("current renderer", selected_label)
        self.assertEqual(selected, [package["root"] / "corona/chair.max"])

    def test_missing_plugins_follow_native_merge_and_do_not_claim_full_success(self):
        worker, calls, responses = self.finish_bridge()
        with patch.dict("sys.modules", {"pymxs": SimpleNamespace(byref=lambda value: value)}):
            worker.finish_merge()
        self.assertEqual(len(calls), 1)
        self.assertIn("autoRenameDups", calls[0][0])
        self.assertIn("renameMtlDups", calls[0][0])
        self.assertEqual(responses[-1]["state"], "complete")
        self.assertTrue(responses[-1].get("incompleteMerge"), "Eksik materyaller tam aktarım gibi sunulmamalı")
        self.assertIn("incomplete", responses[-1]["phase"].lower())
        self.assertTrue(any("Arch & Design" in warning for warning in responses[-1]["warnings"]))
        self.assertTrue(any("incomplete" in warning for warning in responses[-1]["warnings"]))

    def test_declining_conversion_keeps_native_merge_instead_of_blocking_it(self):
        worker, calls, responses = self.finish_bridge(allow_incomplete=False,
            missing=["vrender2015.dlr - VRayMtl", "vrayieslight2015.dlo - VRayIES"])
        worker.rt.Corona = object()
        with patch.dict("sys.modules", {"pymxs": SimpleNamespace(byref=lambda value: value)}):
            worker.finish_merge()
        self.assertEqual(len(calls), 1)
        self.assertEqual(responses[-1]["state"], "complete")
        self.assertTrue(responses[-1]["incompleteMerge"])
        self.assertIsNone(responses[-1]["coronaConversion"])

    def test_corona_conversion_runs_only_after_approval_and_reports_real_counts(self):
        for accepted in (False, True):
            worker, calls, responses = self.finish_bridge(allow_incomplete=accepted, missing=["vrender2015.dlr - VRayMtl"])
            worker.rt.Corona = object()
            original_renderer = worker.rt.renderers.current
            with patch.dict("sys.modules", {"pymxs": SimpleNamespace(byref=lambda value: value)}), patch.object(
                bridge, "convert_imported_to_corona", return_value={"materials": 2, "lights": 1}) as convert:
                worker.finish_merge()
            self.assertEqual(convert.call_count, int(accepted))
            self.assertEqual(responses[-1]["coronaConversion"], {"materials": 2, "lights": 1} if accepted else None)
            self.assertIs(worker.rt.renderers.current, original_renderer)

    def test_viewport_and_legacy_custom_attributes_do_not_block_valid_materials(self):
        worker, calls, responses = self.finish_bridge(allow_incomplete=False, missing=[
            "viewportmanager.gup - ViewportManager", "mrmaterialattribs.gup - mental ray: material custom attribute"])
        with patch.dict("sys.modules", {"pymxs": SimpleNamespace(byref=lambda value: value)}):
            worker.finish_merge()
        self.assertEqual(len(calls), 1)
        self.assertFalse(responses[-1].get("incompleteMerge", False))

    def test_unreadable_models_are_not_retried_or_reported_as_complete(self):
        worker, calls, responses = self.finish_bridge(successful=False)
        with patch.dict("sys.modules", {"pymxs": SimpleNamespace(byref=lambda value: value)}), self.assertRaises(ValueError):
            worker.finish_merge()
        self.assertEqual(len(calls), 1)
        self.assertFalse(any(response["state"] == "complete" for response in responses))
        self.assertTrue(any("Arch & Design" in warning for warning in worker.current["warnings"]))

    def test_live_bitmap_is_relinked_without_touching_existing_scene_maps(self):
        package = self.prepare(self.archive({"chair.max": b"m", "maps/wood.jpg": b"texture"}))
        imported = SimpleNamespace(filename="Z:/old/maps/wood.jpg")
        existing = SimpleNamespace(filename="C:/other/wood.jpg")
        node = object()
        runtime = SimpleNamespace(BitmapTexture=object(),
            getClassInstances=lambda cls, **kwargs: [imported] if kwargs.get("target") is node else [existing],
            getHandleByAnim=lambda obj: id(obj), getPropNames=lambda obj: ["filename"] if hasattr(obj, "filename") else [],
            getProperty=lambda obj, name: getattr(obj, name), setProperty=lambda obj, name, value: setattr(obj, name, value))
        report = bridge.relink_live_files(runtime, [node], package, {}, [])
        self.assertEqual(imported.filename, str(package["root"] / "maps/wood.jpg"))
        self.assertEqual(existing.filename, "C:/other/wood.jpg")
        self.assertEqual(report["verified"], 1)
        self.assertEqual(report["changed"], 1)

    def test_live_ambiguous_map_is_reported_and_not_guessed(self):
        package = self.prepare(self.archive({"chair.max": b"m", "a/wood.jpg": b"a", "b/wood.jpg": b"b"}))
        texture = SimpleNamespace(filename="Z:/wood.jpg")
        runtime = SimpleNamespace(BitmapTexture=object(), getClassInstances=lambda *args, **kwargs: [texture],
            getHandleByAnim=lambda obj: id(obj), getPropNames=lambda obj: ["filename"] if hasattr(obj, "filename") else [],
            getProperty=lambda obj, name: getattr(obj, name), setProperty=lambda obj, name, value: setattr(obj, name, value))
        warnings = []
        report = bridge.relink_live_files(runtime, [object()], package, {}, warnings)
        self.assertEqual(texture.filename, "Z:/wood.jpg")
        self.assertEqual(report["verified"], 0)
        self.assertTrue(warnings)


if __name__ == "__main__":
    unittest.main()
