#!/usr/bin/env python3
"""Create the embedded download catalog from local assets at a pinned Git revision."""
import argparse
import hashlib
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ASSETS = ROOT / 'assets' / 'desktop-pet'

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('revision', help='full commit hash containing the animation files')
    args = parser.parse_args()
    revision = args.revision.lower()
    if len(revision) != 40 or any(c not in '0123456789abcdef' for c in revision):
        raise SystemExit('revision must be a full 40-character commit hash')
    subprocess.run(['git', '-C', str(ROOT), 'cat-file', '-e', revision + '^{commit}'], check=True)
    catalog = json.loads((ASSETS / 'manifest-extra.json').read_text(encoding='utf-8'))
    for index, clip in enumerate(catalog['clips']):
        if index == 0:
            clip['embedded'] = 0
            clip.pop('file', None)
            continue
        name = f'{index:02}.webp' if index < 33 else clip['file']
        path = ASSETS / name
        blob = subprocess.check_output(['git', '-C', str(ROOT), 'show', f'{revision}:assets/desktop-pet/{name}'])
        local = path.read_bytes()
        if local != blob:
            raise SystemExit(f'local asset differs from pinned revision: {name}')
        clip.pop('embedded', None)
        clip['file'] = name
        clip['sha256'] = hashlib.sha256(local).hexdigest()
        clip['size_bytes'] = len(local)
    catalog['repository_commit'] = revision
    (ASSETS / 'download-manifest.json').write_text(json.dumps(catalog, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')

if __name__ == '__main__':
    main()
