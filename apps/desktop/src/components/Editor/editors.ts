import type { Asset } from '../../types';
export type EditorId = 'photo' | 'pdf' | 'vector';
// Biçim listeleri backend'de de doğrulanır; arayüz tek başına disk erişimi yetkisi vermez.
export const editors = {
  photo: { title:'PhotoEditor', engine:'PhotoCraft', path:'/photocraft/index.html', channel:'lumiadam-editor-v1',
    inputs:['jpg','jpeg','png','webp','bmp','tif','tiff','pcraft'], hint:'Select a JPG, PNG, WebP, BMP, TIFF image or .pcraft project in Libraries.' },
  pdf: { title:'PDFEditor', engine:'PdfCraft', path:'/pdfcraft/index.html', channel:'lumiadam-pdf-v1',
    inputs:['pdf'], hint:'Select a PDF in Libraries to read, annotate, fill forms or organize its pages.' },
  vector: { title:'VectorEditor', engine:'VectorCraft', path:'/vectorcraft/index.html', channel:'lumiadam-vector-v1',
    inputs:["svg","svgz","pdf","ai","vectorcraft","drawcraft","jpg","jpeg","png","webp","bmp","tif","tiff","gif"], hint:'Select an SVG, PDF-compatible AI, PDF, vector project or supported image in Libraries.' },
};
export const canOpenInEditor = (asset: Asset | null, editor: EditorId) =>
  !!asset && editors[editor].inputs.includes(asset.extension.toLowerCase());
export const editorForAsset = (asset: Asset | null): EditorId | null => {
  if (!asset) return null;
  if (canOpenInEditor(asset,'photo')) return 'photo';
  if (canOpenInEditor(asset,'pdf')) return 'pdf';
  return canOpenInEditor(asset,'vector') ? 'vector' : null;
};
