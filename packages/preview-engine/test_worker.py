"""İşçiyi geliştirici çalışma dizininden bağımsız gerçek alt süreçte doğrular."""

import json
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image


def main() -> None:
    worker = (
        Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else Path(__file__).with_name("worker.py")
    )
    with tempfile.TemporaryDirectory(prefix="lumiadam-worker-test-") as directory:
        root = Path(directory)
        source, output = root / "Türkçe görsel.png", root / "preview.jpg"
        Image.new("RGB", (32, 16), "blue").save(source)
        requests = [
            {
                "id": "image",
                "method": "image_thumb",
                "params": {"source": str(source), "output": str(output), "size": 8},
            },
            {
                "id": "missing",
                "method": "image_thumb",
                "params": {"source": str(root / "missing.png"), "output": str(output)},
            },
            {"id": "unknown", "method": "unsupported", "params": {}},
        ]
        result = subprocess.run(
            [sys.executable, str(worker)],
            input="\n".join(json.dumps(request) for request in requests) + "\n",
            text=True,
            encoding="utf-8",
            capture_output=True,
            cwd=root,
            timeout=30,
            check=True,
        )
        replies = [json.loads(line) for line in result.stdout.splitlines()]
        assert [reply["id"] for reply in replies] == ["image", "missing", "unknown"]
        assert replies[0]["result"]["ok"] is True
        assert output.is_file()
        with Image.open(output) as thumbnail:
            assert thumbnail.size == (8, 4)
        assert replies[1]["result"]["ok"] is False
        assert "error" in replies[2]
        assert source.is_file()
        print("PASS: isolated worker, preview generation, missing source and unsupported method")


if __name__ == "__main__":
    main()
