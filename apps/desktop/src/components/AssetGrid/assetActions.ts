import { api } from '../../api';
import type { Asset } from '../../types';

// Arayüzdeki bütün editör girişleri backend'in desteklediği aynı formatları kullanır.
export const canEditImage = (asset: Asset | null) => !!asset &&
  ['jpg', 'jpeg', 'png', 'webp', 'bmp', 'tif', 'tiff', 'pcraft'].includes(asset.extension.toLowerCase());

export const isImageAsset = (asset: Asset) =>
  ['jpg', 'jpeg', 'png', 'webp', 'bmp', 'tif', 'tiff', 'tga', 'gif', 'svg', 'avif', 'hdr', 'exr', 'hdri', 'psd', 'psb', 'ai', 'dds', 'ktx', 'ktx2'].includes(asset.extension.toLowerCase());

export async function activateAsset(
  asset: Asset,
  preview: (asset: Asset) => void,
  edit: (asset: Asset) => void | Promise<void>,
): Promise<void> {
  if (isImageAsset(asset)) preview(asset);
  else if (asset.extension.toLowerCase() === 'pcraft') await edit(asset);
  else await api.openWithDefault(asset.filePath);
}
