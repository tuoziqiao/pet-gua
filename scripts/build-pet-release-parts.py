#!/usr/bin/env python3
"""Create deterministic, independently usable Gitee Release animation ZIPs."""
import argparse
import hashlib
import json
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ASSETS = ROOT / 'assets/desktop-pet'
REV = '659177c3e8722d005331bba60cbf29dad4557853'
TAG = 'pet-assets-659177c3'
LIMIT = 80 * 1024 * 1024

def sha(path):
    h = hashlib.sha256()
    with path.open('rb') as f:
        for block in iter(lambda: f.read(1024 * 1024), b''):
            h.update(block)
    return h.hexdigest()

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', type=Path, default=ROOT/'target'/TAG)
    args = parser.parse_args()
    manifest_path = ASSETS/'download-manifest.json'
    catalog = json.loads(manifest_path.read_text(encoding='utf-8'))
    if catalog.get('repository_commit') != REV: raise SystemExit('download manifest revision mismatch')
    files = sorted((c for c in catalog['clips'] if 'file' in c), key=lambda c:c['file'])
    if len(files) != 113 or len({c['file'] for c in files}) != 113: raise SystemExit('expected 113 unique animation files')
    parts, group, size = [], [], 22
    for clip in files:
        path=ASSETS/clip['file']
        if not path.is_file() or path.stat().st_size != clip['size_bytes'] or sha(path) != clip['sha256']:
            raise SystemExit(f'animation missing or corrupt: {path}')
        # ZIP_STORED: local and central headers plus the filename in each.
        item_size=clip['size_bytes']+76+2*len(clip['file'].encode('utf-8'))
        if item_size+22>LIMIT: raise SystemExit(f'animation exceeds part limit: {path}')
        if group and size+item_size>LIMIT:
            parts.append(group); group=[]; size=22
        group.append(clip); size+=item_size
    if group: parts.append(group)
    args.output.mkdir(parents=True,exist_ok=True)
    names=[f'pet-animations-659177c3-{i:02}.zip' for i in range(1,len(parts)+1)]
    if any((args.output/name).exists() for name in names) or (args.output/'bundle-manifest.json').exists():
        raise SystemExit(f'output already exists; choose a new --output: {args.output}')
    metadata=[]
    for name,group in zip(names,parts):
        path=args.output/name
        with zipfile.ZipFile(path,'x',compression=zipfile.ZIP_STORED,allowZip64=False) as archive:
            for clip in group:
                info=zipfile.ZipInfo(clip['file'],(2020,1,1,0,0,0))
                info.compress_type=zipfile.ZIP_STORED
                info.external_attr=0o600<<16
                archive.writestr(info,(ASSETS/clip['file']).read_bytes())
        if path.stat().st_size>LIMIT: raise SystemExit(f'ZIP exceeds limit: {path}')
        metadata.append(dict(name=name,size_bytes=path.stat().st_size,sha256=sha(path),files=[c['file'] for c in group]))
        print(f'{name}: {path.stat().st_size} bytes, {len(group)} files',flush=True)
    release=dict(tag=TAG,parts=metadata)
    (args.output/'bundle-manifest.json').write_text(json.dumps(release,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
    catalog['release']=release
    manifest_path.write_text(json.dumps(catalog,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
    print(f'Upload {len(parts)} ZIP files from {args.output} to Gitee Release {TAG}',flush=True)

if __name__ == '__main__': main()
