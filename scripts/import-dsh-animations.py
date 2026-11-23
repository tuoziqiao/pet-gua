#!/usr/bin/env python3
"""Convert the pinned upstream animation catalog into the local offline asset pack."""
import argparse
import concurrent.futures
import hashlib
import json
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
REV = '572cf0ff68e7ffbe76afe4ba5fd8ae99ec7e0733'
OLD_REV = '33938a355f422525886c3facd883d7179c58ebf4'

def jsonc(text):
    out, i, quoted, escaped = [], 0, False, False
    while i < len(text):
        c = text[i]
        if quoted:
            out.append(c)
            if escaped: escaped = False
            elif c == '\\': escaped = True
            elif c == '"': quoted = False
            i += 1
        elif c == '"':
            quoted = True; out.append(c); i += 1
        elif text[i:i+2] == '//':
            end = text.find('\n', i)
            i = len(text) if end < 0 else end
        else:
            out.append(c); i += 1
    return json.loads(''.join(out))

def digest(data): return hashlib.sha256(data).hexdigest()

def webp_info(path):
    data = path.read_bytes()
    if data[:4] != b'RIFF' or data[8:12] != b'WEBP' or int.from_bytes(data[4:8], 'little') + 8 != len(data):
        raise ValueError(f'invalid WebP: {path}')
    pos, durations, alpha, size = 12, [], False, None
    while pos + 8 <= len(data):
        tag, length = data[pos:pos+4], int.from_bytes(data[pos+4:pos+8], 'little')
        payload = data[pos+8:pos+8+length]
        if len(payload) != length: raise ValueError(f'truncated WebP: {path}')
        if tag == b'VP8X' and length >= 10:
            size=(1+int.from_bytes(payload[4:7],'little'),1+int.from_bytes(payload[7:10],'little'))
        if tag == b'ANMF' and length >= 16:
            durations.append(int.from_bytes(payload[12:15], 'little'))
            alpha |= b'ALPH' in payload[16:] or b'VP8L' in payload[16:]
        pos += 8 + length + (length & 1)
    if not durations or not all(durations) or not alpha or size != (640,360): raise ValueError(f'invalid animation timing, alpha or canvas: {path}')
    return durations

def main():
    p = argparse.ArgumentParser()
    p.add_argument('source', type=Path)
    p.add_argument('--ffmpeg', required=True)
    p.add_argument('--destination', type=Path, default=ROOT/'assets/desktop-pet')
    p.add_argument('--jobs', type=int, default=3)
    args = p.parse_args()
    commit = subprocess.check_output(['git','-C',str(args.source),'rev-parse','HEAD'], text=True).strip()
    if commit != REV: raise SystemExit(f'Expected {REV}, got {commit}')
    conf = jsonc((args.source/'dsh-pet/assets/config.jsonc').read_text(encoding='utf-8'))
    anim = conf['animations']; source = args.source/'dsh-pet/assets/webm'
    old = json.loads((ROOT/'assets/desktop-pet/manifest.json').read_text(encoding='utf-8'))['clips']
    records, hashes, groups, jobs = [], {}, {}, {}
    for i, item in enumerate(old):
        raw = subprocess.check_output(['git','-C',str(args.source),'show',f'{OLD_REV}:dsh-pet/assets/webm/{item["source"]}.webm'])
        hashes[digest(raw)] = i
        records.append(dict(name=item['source'], embedded=i, durations_ms=item['durations_ms'], no_mirror=item['no_mirror'], category=item['category'], anchor=item['anchor'], size=item['size'], loop=item['loop']))
    args.destination.mkdir(parents=True, exist_ok=True)
    def add(name, group, no_mirror=False):
        path = source/(name+'.webm')
        if not path.is_file(): raise FileNotFoundError(path)
        sha = digest(path.read_bytes())
        if sha not in hashes:
            hashes[sha] = len(records)
            records.append(dict(name=name, file=sha+'.webp', source_sha256=sha, no_mirror=no_mirror, category=group, anchor=[0.5,1.0], size=[640,360], loop=group in ('待机','拖拽')))
            jobs[sha+'.webp'] = path
        idx = hashes[sha]
        records[idx]['no_mirror'] |= no_mirror
        groups.setdefault(group, [])
        if idx not in groups[group]: groups[group].append(idx)
    for key, label in [('idle','待机'),('turn','转向'),('drag','拖拽'),('clicks','点击回应')]:
        for name in anim[key]: add(name,label)
    for move in anim['moves']['actions']: add(move['name'],'移动')
    for category in anim['categories']:
        for name in category['actions']: add(name,category['id'],category.get('noMirror',False))
    for key,label in [('balance','余额档位'),('whisper','碎碎念'),('workStatus','工作状态')]:
        for slot in anim['events'][key]:
            for name in slot if isinstance(slot,list) else [slot]: add(name,label)
    for sleep in anim['sleep']:
        for phase in ('start','loop','end'):
            for name in sleep[phase]: add(name,'睡觉')
    for name in anim.get('baifan',[]): add(name,'玩法')
    # Keep old-only clips visible in their existing groups.
    labels={'idle':'待机','turn':'转向','drag':'拖拽','click':'点击回应','move':'移动','idle_action':'小动作','play':'玩耍','eat':'吃什么','season':'时节','text':'文字','talk':'碎碎念','sleep_start':'睡觉','sleep_loop':'睡觉','sleep_end':'睡觉'}
    for i,item in enumerate(old):
        label=labels.get(item['category'],'小动作')
        if not any(i in indices for indices in groups.values()): groups.setdefault(label,[]).append(i)
    def convert(job):
        name, src = job; dest = args.destination/name
        if not dest.exists():
            cached = ROOT/'target/desktop-pet-characters'/name
            if cached.is_file(): shutil.copyfile(cached,dest)
            else:
                tmp=dest.with_suffix('.tmp.webp')
                subprocess.run([args.ffmpeg,'-hide_banner','-loglevel','error','-y','-threads','1','-c:v','libvpx-vp9','-i',str(src),'-an','-c:v','libwebp_anim','-lossless','0','-quality','85','-loop','0',str(tmp)],check=True)
                tmp.replace(dest)
        return name, webp_info(dest)
    with concurrent.futures.ThreadPoolExecutor(max_workers=max(1,args.jobs)) as pool:
        converted=dict(pool.map(convert,jobs.items()))
    for record in records:
        if 'file' in record:
            record['durations_ms']=converted[record['file']]
            output=args.destination/record['file']
            record['sha256']=digest(output.read_bytes())
            record['size_bytes']=output.stat().st_size
    manifest=dict(upstream_commit=REV,clips=records,groups=groups,weights=conf['animationWeights'],
                  category_weights={c['id']:c['weight'] for c in anim['categories']}, moves=anim['moves'])
    (args.destination/'manifest-extra.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
    print(f'{len(records)} clips, {len(jobs)} external files: {args.destination}',flush=True)

if __name__ == '__main__': main()
