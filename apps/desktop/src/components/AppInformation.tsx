import { t as tr, useTranslation, translateMessage } from '../i18n';
import { useEffect, useRef, useState, type MouseEvent } from 'react';
import { getVersion } from '@tauri-apps/api/app';
import { isTauri } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-shell';
import { version as packageVersion } from '../../package.json';
import styles from './StorageSettings.module.css';

export type InformationPage = 'help' | 'about' | 'faq';

const titles = { help: 'Help', about: 'About LumiaDAM', faq: 'Frequently Asked Questions' };
const links = {
  help: [
    ['Documentation', 'https://github.com/yuxelmurat/LumiaDAM#readme'],
    ['Report an issue', 'https://github.com/yuxelmurat/LumiaDAM/issues'],
  ],
  about: [
    ['LumiaDAM on GitHub', 'https://github.com/yuxelmurat/LumiaDAM'],
    ['OpenDAM', 'https://github.com/LawLaw09/OpenDAM'],
    ['PhotoCraft', 'https://github.com/storytold/photocraft'],
    ['PdfCraft', 'https://github.com/storytold/pdfcraft'],
    ['VectorCraft', 'https://github.com/storytold/vectorcraft'],
    ['CADCraft', 'https://github.com/storytold/cadcraft'],
    ['Exav Viewer', 'https://github.com/sylvinus/exav'],
    ['Buy me a coffee', 'https://buymeacoffee.com/yuxelmurat8'],
  ],
  faq: [['Documentation', 'https://github.com/yuxelmurat/LumiaDAM#readme']],
};

export function AppInformation({ page, onClose }: { page: InformationPage; onClose: () => void }) {
  useTranslation();
  const dialog = useRef<HTMLDialogElement>(null);
  const [version, setVersion] = useState(packageVersion);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    dialog.current?.showModal();
    if (page !== 'about' || !isTauri()) return;
    let active = true;
    getVersion().then((value) => { if (active) setVersion(value); })
      .catch((reason) => { if (active) setError(`Could not read the app version: ${String(reason)}`); });
    return () => { active = false; };
  }, [page]);

  const openLink = async (event: MouseEvent<HTMLAnchorElement>) => {
    // Dış bağlantılar uygulama sayfasının yerine geçmeden sistem tarayıcısında açılır.
    event.preventDefault();
    setError(null);
    try { await open(event.currentTarget.href); }
    catch (reason) { setError(`Could not open the link: ${String(reason)}`); }
  };

  return <dialog ref={dialog} className={styles.dialog} aria-labelledby="app-information-title" onClose={onClose}>
    <h2 id="app-information-title">{tr(titles[page])}</h2>
    {page === 'help' && <>
      <h3>{tr("Getting started")}</h3>
      <p>{tr("Choose Libraries from the LumiaDAM menu, then use Add Library to select a drive or folder. The first scan builds a catalog; Status shows scan and thumbnail progress.")}</p>
      <p>{tr("Search matches both file names and folder paths. Try kitchen corona to find files in matching folders. Type filters still apply. Use Files and Folders to switch between results; open a folder to browse inside.")}</p>
      <h3>{tr("PhotoEditor")}</h3>
      <p>{tr("Double-click an image for a larger preview, a PDF for PDFEditor, or a vector project for VectorEditor. Other files open their default application. Windows file associations determine which installed program opens ZIP, MAX, and other formats.")}</p>
      <p>{tr("Right-click a JPG, PNG, WebP, BMP, TIFF image, or .pcraft project, then choose Open in PhotoEditor. You can also select the file and choose PhotoEditor from the LumiaDAM menu. The editor fills the workspace. Libraries keeps the session open; Close editor asks before discarding it. File > Export saves a new image beside the original and adds it to the library. File > Save As saves a layered .pcraft project.")}</p>
      <h3>{tr("PDFEditor and VectorEditor")}</h3>
      <p>{tr("PDFEditor uses PdfCraft for PDF reading, comments, forms and page organization. VectorEditor uses VectorCraft for SVG artwork, vector projects and PDF-compatible AI files. Open them from the LumiaDAM menu or the file's right-click menu. Save/export creates a new library asset; wait for the LumiaDAM Saved message before closing. Both engines are in active development.")}</p>
      <h3>{tr("CAD Viewer")}</h3>
      <p>{tr("Double-click a DWG or DXF file, or choose Open in CAD Viewer from its right-click menu. The local viewer supports layers, model and paper layouts, pan, and zoom without uploading the drawing. The source drive must be available when opening the full drawing; cached thumbnails remain visible offline.")}</p>
      <h3>{tr("CADCraft")}</h3>
      <p>{tr("Right-click a DWG or DXF and choose Open in CADCraft to edit a copy in the full-page experimental CAD editor. Saving creates a new catalog file and preserves the source. CADCraft is pre-alpha; unsupported DWG entities may be lost in the edited copy, so keep the original until you verify the result.")}</p>
      <h3>{tr("Keyboard shortcuts in Libraries")}</h3>
      <p>{tr("Enter opens the focused file. Esc closes an enlarged preview. Hold Space for Quick Look. Ctrl+C copies selected file paths. F toggles favorites. F2 renames the focused file. F5 refreshes libraries. Delete asks before moving files to the Recycle Bin.")}</p>
      <h3>{tr("Catalog storage")}</h3>
      <p>{tr("Settings lets you choose where catalogs and thumbnails are stored. A local SSD is recommended; source files remain in their original library folders.")}</p>
    </>}
    {page === 'about' && <>
      <p><strong>LumiaDAM{version ? ` ${version}` : ''}</strong><br />
        {tr("Local asset management and image editing for architects, designers, and 3D artists.")}</p>
      <p>{tr("Developed by yuxelmurat. LumiaDAM is free and open source under the MIT License.")}</p>
      <h3>{tr("Credits")}</h3>
      <p>{tr("Based on OpenDAM by OpenDAM Contributors. The original MIT copyright notice is preserved.")}</p>
      <p>{tr("The embedded editors are based on PhotoCraft, PdfCraft and VectorCraft, licensed under MIT OR Apache-2.0. Their license and attribution files are included in packaged builds.")}</p>
      <p>{tr("The experimental CAD editor embeds CADCraft, dual-licensed under MIT or Apache-2.0. Its license and attribution notices are included in packaged builds.")}</p>
      <p>{tr("CAD viewing and embedded drawing previews use the MIT-licensed Exav Viewer and exav-render projects.")}</p>
    </>}
    {page === 'faq' && <>
      <details open><summary>{tr("Does every search scan my NAS again?")}</summary>
        <p>{tr("No. Search and filters query the catalog after the first scan. Initial scans, new thumbnails, opening files, and rescanning still depend on your disk and network speed.")}</p></details>
      <details><summary>{tr("Where are catalogs and thumbnails stored?")}</summary>
        <p>{tr("In LumiaDAM's local application data folder by default, with a separate subfolder per library. Choose another location in Settings. A storage change takes effect after restarting the app.")}</p></details>
      <details><summary>{tr("How do files with numbered names appear in search?")}</summary>
        <p>{tr("Search also checks the full folder path. A file named 2542154.zip in a kitchen/corona folder can match kitchen corona. Corona here comes from the folder name, not analysis of the model.")}</p></details>
      <details><summary>{tr("Why does a ZIP or MAX file have a related image?")}</summary>
        <p>{tr("LumiaDAM can match a companion image in the same folder, such as 9.zip and 9.jpg. The small Related image frame identifies that association. It does not extract or preview ZIP contents.")}</p></details>
      <details><summary>{tr("Can I preview HDR and EXR files?")}</summary>
        <p>{tr("HDR and EXR use Windows thumbnail providers when available, with a built-in image decoder as fallback. Support depends on the file's encoding and installed codecs. Previews are saved in the local catalog cache; these formats are not supported for editing in PhotoEditor.")}</p></details>
      <details><summary>{tr("Does editing overwrite my original image?")}</summary>
        <p>{tr("No. The LumiaDAM save/export bridge creates a new file beside the source and adds it to the catalog. This applies to images, PDFs and vectors. Use File > Save As to keep a layered .pcraft or .vectorcraft project.")}</p></details>
      <details><summary>{tr("Can VectorEditor open every Illustrator file?")}</summary>
        <p>{tr("No. AI import requires a PDF-compatible file. SVG, PDF and VectorCraft projects are also supported; complex artwork may render differently. PdfCraft and VectorCraft are still in development.")}</p></details>
      <details><summary>{tr("Which CAD drawings can LumiaDAM open?")}</summary>
        <p>{tr("CAD Viewer opens DWG R13–R2018 and DXF drawings locally. It provides model and paper layouts, layer visibility, pan and zoom. External references, proxy objects, or newer DWG versions may be incomplete. Embedded drawing previews are cached locally when the source includes one.")}</p></details>
      <details><summary>{tr("Can I edit a DWG in LumiaDAM?")}</summary>
        <p>{tr("CADCraft opens DWG and DXF drawings for experimental editing. Save creates a new catalog copy and does not overwrite the source. CADCraft is pre-alpha and may drop drawing entities it does not support; verify edited files in your main CAD application before relying on them.")}</p></details>
      <details><summary>{tr("Can I use a disconnected library?")}</summary>
        <p>{tr("Existing catalog entries and cached thumbnails remain available. Opening source files, editing, generating missing previews, and rescanning require access to the source drive or NAS. Preview generation pauses when the source is unavailable and retries automatically. Status shows this waiting state.")}</p></details>
      <details><summary>{tr("Is the 3ds Max render bridge available?")}</summary>
        <p>{tr("The optional 3ds Max plugin currently merges MAX models and ZIP/RAR archives into an open scene, collects their files in the project and updates model asset references. Install it through the LumiaDAM installer and restart 3ds Max. Direct render transfer is planned.")}</p></details>
    </>}
    <div className={styles.actions}>
      {links[page].map(([label, url]) => <a key={url} href={url} target="_blank" rel="noreferrer"
        style={{ color: 'var(--color-brand-300)' }} onClick={openLink}>{tr(label)}</a>)}
    </div>
    {error && <p role="alert">{translateMessage(error)}</p>}
    <div className={styles.actions}><button type="button" onClick={() => dialog.current?.close()}>{tr("Close")}</button></div>
  </dialog>;
}
