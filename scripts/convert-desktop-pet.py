#!/usr/bin/env python3
"""Convert the pinned dsh-pet VP9-alpha clips to animated WebP.

Requires ffmpeg with libvpx-vp9 and libwebp_anim, plus Pillow for validation.
The source checkout and build tools are inputs; the generated files are written
only to the explicitly supplied destination.
"""
import argparse
import json
import subprocess
from pathlib import Path

from PIL import Image


UPSTREAM_COMMIT = "33938a355f422525886c3facd883d7179c58ebf4"
CLIPS = [
    ("idle", "待机呼吸休闲"), ("turn", "东张西望"),
    ("drag", "被鼠标拖拽悬空反馈"),
    ("click", "点击回应-开心跃动"), ("click", "点击回应-害羞惊讶"),
    ("click", "点击回应-傲娇生气"), ("click", "点击回应-挠痒咯咯笑"),
    ("click", "点击回应-元气挥手"),
    ("move", "螃蟹走路"), ("move", "原地漂浮踏步"),
    ("move", "原地左转奔跑"),
    ("idle_action", "悠闲哼歌"), ("idle_action", "超大伸懒腰"),
    ("idle_action", "哈欠连天"), ("idle_action", "晨间刷牙"),
    ("play", "原地专心玩魔方"), ("play", "原地蹲下玩玩具汽车"),
    ("play", "鲸鱼吐泡泡特效"), ("play", "玩水枪"),
    ("eat", "吃白饭"), ("eat", "吃冰淇淋融化"), ("eat", "涮火锅"),
    ("season", "被落叶淹没"), ("season", "堆雪人"), ("season", "放烟花"),
    ("text", "是啊，吃什么"), ("text", "深度思考碎碎念"),
    ("talk", "碎碎念-擦桌碎碎念"), ("talk", "碎碎念-发呆碎碎念"),
    ("talk", "碎碎念-对屏碎碎念"),
    ("sleep_start", "睡觉-开始"), ("sleep_loop", "睡觉-循环"),
    ("sleep_end", "睡觉-起来"),
]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path, help="pinned dsh-pet checkout")
    parser.add_argument("destination", type=Path)
    parser.add_argument("--ffmpeg", default="ffmpeg")
    args = parser.parse_args()
    actual = subprocess.check_output(
        ["git", "-C", str(args.source), "rev-parse", "HEAD"], text=True
    ).strip()
    if actual != UPSTREAM_COMMIT:
        raise SystemExit(f"Expected upstream {UPSTREAM_COMMIT}, got {actual}")
    source = args.source / "dsh-pet" / "assets" / "webm"
    args.destination.mkdir(parents=True, exist_ok=True)
    manifest = []
    for index, (category, name) in enumerate(CLIPS):
        src = source / f"{name}.webm"
        if not src.is_file():
            raise FileNotFoundError(src)
        output = args.destination / f"{index:02d}.webp"
        subprocess.run([
            args.ffmpeg, "-hide_banner", "-loglevel", "error", "-y",
            "-c:v", "libvpx-vp9", "-i", str(src), "-an", "-c:v", "libwebp_anim",
            "-lossless", "0", "-quality", "85", "-loop", "0", str(output),
        ], check=True)
        with Image.open(output) as image:
            if not getattr(image, "is_animated", False):
                raise ValueError(f"Not animated: {output}")
            durations = []
            alpha_min = 255
            for frame in range(image.n_frames):
                image.seek(frame)
                rgba = image.convert("RGBA")
                alpha_min = min(alpha_min, rgba.getchannel("A").getextrema()[0])
                durations.append(image.info.get("duration", 0))
            if alpha_min == 255 or not all(d > 0 for d in durations):
                raise ValueError(f"Missing alpha or frame duration: {output}")
            manifest.append({"file": output.name, "source": name, "category": category,
                             "loop": category in ("idle", "drag", "sleep_loop"),
                             "no_mirror": category == "text", "anchor": [0.5, 1.0],
                             "size": list(image.size), "durations_ms": durations})
        print(f"{index + 1}/{len(CLIPS)} {name}", flush=True)
    (args.destination / "manifest.json").write_text(
        json.dumps({"upstream_commit": UPSTREAM_COMMIT, "clips": manifest},
                   ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
