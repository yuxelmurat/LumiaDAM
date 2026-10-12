import { api } from '../../api';
import type { Asset } from '../../types';
import { editorForAsset, type EditorId } from '../Editor/editors';

export const isImageAsset = (asset: Asset) =>
  ['jpg', 'jpeg', 'png', 'webp', 'bmp', 'tif', 'tiff', 'tga', 'gif', 'svg', 'avif', 'hdr', 'exr', 'hdri', 'psd', 'psb', 'ai', 'dds', 'ktx', 'ktx2'].includes(asset.extension.toLowerCase());

export async function activateAsset(
  asset: Asset,
  preview: (asset: Asset) => void,
  edit: (asset: Asset, editor?: EditorId) => void | Promise<void>,
): Promise<void> {
  if (isImageAsset(asset)) preview(asset);
  else if (['pcraft', 'pdf', 'vectorcraft', 'drawcraft', 'svgz'].includes(asset.extension.toLowerCase()))
    await edit(asset, editorForAsset(asset) ?? 'photo');
  else await api.openWithDefault(asset.filePath);
}
