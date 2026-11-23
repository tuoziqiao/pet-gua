# 人形桌宠素材来源与转换

素材来自 [dsh-pet](https://github.com/PC2005-cloud/dsh-pet)：原有内嵌素材固定提交
`33938a355f422525886c3facd883d7179c58ebf4`，扩展动画固定提交
`572cf0ff68e7ffbe76afe4ba5fd8ae99ec7e0733`。实施时远程查询失败，无法确认该本地提交是否为上游最新版本。
其角色是社区二创的 DeepSeek
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

转换脚本验证素材来自指定提交，并检查输出动画的帧时长与透明通道。转换修改了编码格式和画质，未改变角色原型授权。

内置的甩抛落地晕眩动画 `landing-dizzy.webp` 单独来自
[dsh-pet PR #400](https://github.com/PC2005-cloud/dsh-pet/pull/400) 的提交
`af8ca08f9893aba6ce1101a2f10eb7e7f24c36b8`，原始 `甩出落地晕眩.webm`
的 Git blob 为 `1a185a8a862251d491925fd50f8db14e6cf3db5e`。该素材通过
FFmpeg 的 `libvpx-vp9` 解码并以 `libwebp_anim`、质量 85 转为透明动画 WebP；
它随程序内置，不依赖完整动画素材包。

弹弓图层素材 `sling.png`、`band.png`、`pouch.png` 和 `bubble-s1.png` 至
`bubble-s3.png` 来自 [dsh-pet PR #158](https://github.com/PC2005-cloud/dsh-pet/pull/158)
的固定提交 [`3d20ab084ccd0a68312d441613201b2ed70b3e2e`](https://github.com/kanoha222/dsh-pet/tree/3d20ab084ccd0a68312d441613201b2ed70b3e2e/dsh-pet/runtime/electron-helper/game-assets)，
源路径为 `dsh-pet/runtime/electron-helper/game-assets/`。这些 PNG 随程序内置；皮筋和弹弓按该提交
的 `sling.json` 中的尺寸及两个叉端位置绘制。

扩展素材由 `scripts/import-dsh-animations.py` 从固定提交的动画配置生成，默认存放在
`assets/desktop-pet`，扩展清单为 `manifest-extra.json`，与原有内嵌清单分开保存。
其中已有内嵌动作通过源文件 SHA-256 复用，其余动作按需
从磁盘解码；转换需要 FFmpeg，应用运行时不需要。运行转换：

```powershell
python scripts/import-dsh-animations.py ../dsh-pet --ffmpeg <ffmpeg.exe>
```

安装包和便携包内嵌 `00.webp` 待机动画、`landing-dizzy.webp` 晕眩动画及动作目录。用户可在设置页主动下载完整动画，
素材保存在 `%APPDATA%\gua\desktop-pet-assets`（设置 `GUA_DATA_DIR` 时位于该目录下）。
下载来源是 Gitee Release 标签 `pet-assets-659177c3`，对应素材提交
`659177c3e8722d005331bba60cbf29dad4557853`。程序通过 Release API 按附件名查找七个
ZIP，使用内嵌的 `download-manifest.json` 校验每个包及包内动画的大小和 SHA-256。
启动时在后台扫描正式目录和 `.partial` 目录，已校验动画无需等待整套下载即可播放，
也可在设置页手动重新扫描。继续下载时会复用已有动画；某个 ZIP 中仍有缺失文件时需要下载整包。
中断后保留已校验的分包和动画；启用已有动画无需网络。用户数据在升级和卸载时保留。
下载及解压期间会同时存放 ZIP 和已解压素材，需要约 1 GiB 的临时空间；成功启用前会清理 ZIP。

更新动画下载版本时，先将素材提交并推送到 Gitee，再使用包含这些素材的完整提交号生成
可信下载清单：`python scripts/generate-pet-download-manifest.py <40位提交号>`。
脚本会核对本地素材与该提交中的内容；同时需将下载器和分包脚本中的固定提交号、Release 标签和包名更新为同一版本。
执行 `python scripts/build-pet-release-parts.py` 会在 `target/pet-assets-659177c3` 生成
七个不超过 80 MiB 的独立 ZIP 及 `bundle-manifest.json`，并将分包校验信息写入内嵌清单。
在 Gitee 创建公开 Release `pet-assets-659177c3` 后，手动上传七个 ZIP；`bundle-manifest.json`
供发布者核对，不作为应用的远程信任来源。分包生成后不可重新打包或改名，否则哈希会变化。
上传后需验证未登录访问、附件链接和重定向；下载失败会在设置页显示原因。
