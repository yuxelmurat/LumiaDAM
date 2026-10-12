"""Gerçek önizleme motorunun yerel AVIF ve video çıktısını doğrular."""

import importlib.util
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

from PIL import Image, features

spec = importlib.util.spec_from_file_location(
    "worker", Path(__file__).resolve().parents[1] / "worker.py"
)
worker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(worker)


class CachedPreviews(unittest.TestCase):
    @unittest.skipUnless(features.check("avif"), "Pillow AVIF codec is required")
    def test_avif_creates_small_local_png(self):
        with tempfile.TemporaryDirectory() as folder:
            source, output = Path(folder) / "source.avif", Path(folder) / "cache/thumbnail.png"
            Image.new("RGB", (900, 600), "cyan").save(source)
            result = worker.image_thumb(str(source), str(output), 512)
            self.assertTrue(result["ok"], result)
            source.unlink()
            with Image.open(output) as thumb:
                self.assertEqual(thumb.size, (512, 341))

    @unittest.skipUnless(shutil.which("ffmpeg"), "FFmpeg is required for video thumbnails")
    def test_video_creates_local_jpeg_without_open_player(self):
        with tempfile.TemporaryDirectory() as folder:
            source, output = Path(folder) / "source.mp4", Path(folder) / "cache/thumbnail.jpg"
            subprocess.run(
                [
                    "ffmpeg",
                    "-loglevel",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "color=c=cyan:s=32x32:d=0.2",
                    "-y",
                    str(source),
                ],
                check=True,
                capture_output=True,
            )
            result = worker.video_thumb(str(source), str(output), "00:00:00")
            self.assertTrue(result["ok"], result)
            source.unlink()
            with Image.open(output) as thumb:
                self.assertEqual(thumb.format, "JPEG")


if __name__ == "__main__":
    unittest.main()
