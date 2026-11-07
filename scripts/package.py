"""Create a source + Windows executable archive, excluding build/scratch data."""
from pathlib import Path
from zipfile import ZipFile, ZIP_DEFLATED
import hashlib
import json
import re
import subprocess

root = Path(__file__).resolve().parents[1]
version = re.search(r'^version = "([^"]+)"', (root / "Cargo.toml").read_text(), re.M).group(1)
destination = root.parent / f"SilhouetteTetris-{version}-Windows-x64.zip"
release = root / "target/release/silhouette-tetris.exe"
help_text = subprocess.run([str(release), "--help"], cwd=root, capture_output=True, text=True, check=True).stdout
assert help_text.startswith(f"SilhouetteTetris {version}\n"), "Build the current release before packaging"
excluded = {"target", ".git", "logs", "__pycache__"}
files = sorted(
    p for p in root.rglob("*")
    if p.is_file()
    and not any(part in excluded for part in p.relative_to(root).parts)
    and not p.name.startswith("work-")
    and p.name != "local.toml"
    and p.name != "SHA256SUMS.json"
    and not (p.relative_to(root).parts[0] == "config" and p.name != "default.toml")
)
def source(path):
    # Do not overwrite the user's running executable to create an update ZIP.
    return release if path == root / "SilhouetteTetris.exe" else path
hashes = {
    p.relative_to(root).as_posix(): hashlib.sha256(source(p).read_bytes()).hexdigest()
    for p in files
}
with ZipFile(destination, "w", ZIP_DEFLATED, compresslevel=9) as archive:
    for path in files:
        archive.write(source(path), "silhouette-tetris/" + path.relative_to(root).as_posix())
    archive.writestr("silhouette-tetris/SHA256SUMS.json", json.dumps(hashes, indent=2, ensure_ascii=False) + "\n")
print(json.dumps({"archive": str(destination), "bytes": destination.stat().st_size,
                  "files": len(files)+1}, ensure_ascii=False))
