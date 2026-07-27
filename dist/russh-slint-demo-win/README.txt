russh-slint-demo (Windows x86_64) - v2 品质对齐版
==================================================

本次版本针对前一版做了 12 项品质修复，详见下方"本次修复"。

构建信息
--------
- 目标三元组：x86_64-pc-windows-gnu
- 编译方式：cargo build --release --target x86_64-pc-windows-gnu
- 依赖：仅依赖 Windows 系统自带 DLL（kernel32/gdi32/opengl32/imm32/ole32 等），无 mingw 运行时依赖

文件清单
--------
- russh-slint-demo.exe   程序主执行文件 (44 MB)
- config.json            SSH 连接配置（编辑后再运行程序）
- README.txt             本说明

使用方法
--------
1. 编辑 config.json，填入真实 SSH 信息：
   {
     "host": "192.168.1.10",
     "port": 22,
     "username": "your_user",
     "password": "your_password"
   }
2. 双击 russh-slint-demo.exe 启动
3. 启动后会显示连接遮罩，实时显示进度：
   正在连接到 host:port → 正在认证 → 正在打开 channel → 正在请求 PTY → 正在请求 shell
4. 连接成功后遮罩撤销，切换到终端视图，可正常输入命令
5. 关闭窗口即断开 SSH 连接

本次修复（v2 相对 v1）
----------------------
1. DPI 自适应：HiDPI 显示器（如 2x 缩放）下字体不再模糊，字符物理分辨率与屏幕像素 1:1
2. 字体大小对齐 Windows Terminal 默认 12pt：CELL_W=9, CELL_H=19, font_size=18.0
3. 窗口默认尺寸 1000×640，最小 400×300
4. 渲染区域精确填充窗口，无 letterbox 黑边
5. 渲染性能优化：空闲时不再每帧上传整屏像素，htop 刷新时帧率显著提升
6. 状态栏文本 dirty 检查：不再每帧 format! + set
7. SSH 关闭后清屏：不再残留最后一帧画面
8. app_cursor 模式跟踪：vim/less 中方向键不再插入垃圾字符（A/B/C/D）
9. Ctrl+Right / Ctrl+Left 等 modifyOtherKeys 编码：vim 中可正确跳单词
10. Shift+Tab 输出 backtab (\x1b[Z)
11. Ctrl+Shift+V 粘贴剪贴板内容（按 bracketed paste 模式）
12. Ctrl+非字母字符控制编码补全（C-@、C-[、C-\、C-]、C-^、C-_）
13. htop 中滚轮可滚动屏幕（鼠标跟踪模式启用时转发滚轮事件给远端）
14. 右侧滚动条显示 scrollback 位置

快捷键
------
- Ctrl+Shift+V   粘贴剪贴板
- 鼠标中键       粘贴剪贴板
- 鼠标左键拖拽   选区（释放后自动复制到剪贴板）
- 滚轮           非 htop 模式下滚动 scrollback；htop 模式下转发给远端
- Shift+PageUp/Down  滚动 scrollback
- Ctrl+C / Ctrl+D    按终端语义发送（Ctrl+D 在 bash 中触发 EOF）

注意事项
--------
- 程序会在启动时从同目录读取 config.json
- 若 config.json 缺失或格式错误，遮罩会显示错误信息
- 终端支持滚轮 scrollback、鼠标选区/粘贴、窗口 resize
