# 人形桌宠素材来源与转换

素材来自 [dsh-pet](https://github.com/PC2005-cloud/dsh-pet)，固定提交
`33938a355f422525886c3facd883d7179c58ebf4`。其角色是社区二创的 DeepSeek
「大肥鱼」；原型「溟月」出自上善无形，女仆鲸鱼娘设计出自 ZipZipPipe。
上游说明角色形象遵循 CC BY-NC-SA 4.0（署名、非商业、相同方式共享），
并要求衍生作品公开渠道附上原项目地址。项目代码 MIT 许可不改变素材的许可。
具体使用条件以[上游许可说明](https://github.com/PC2005-cloud/dsh-pet#许可)为准。

`scripts/convert-desktop-pet.py` 是离线转换实验：它将所选透明 VP9 WebM
转为动画 WebP，并记录素材来源、分类、尺寸、逐帧时长和显示标记。
运行需要 Python、Pillow、Git 及包含 `libvpx-vp9` 解码器和 `libwebp_anim`
编码器的 FFmpeg：

```powershell
python scripts/convert-desktop-pet.py <dsh-pet-checkout> <output-directory> --ffmpeg <ffmpeg.exe>
```

转换脚本验证素材来自指定提交，并检查输出动画的帧时长与透明通道。转换结果
随程序嵌入可执行文件；转换修改了编码格式和画质，未改变角色原型授权。
