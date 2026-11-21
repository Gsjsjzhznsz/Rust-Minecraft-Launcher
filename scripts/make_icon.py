"""生成 Quartz 应用图标（1024x1024 PNG）。"""
from PIL import Image, ImageDraw, ImageFont

SIZE = 1024
img = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
d = ImageDraw.Draw(img)

# 圆角方块底（石英绿）
d.rounded_rectangle([48, 48, SIZE - 48, SIZE - 48], radius=200, fill=(16, 185, 129, 255))
# 内描边
d.rounded_rectangle(
    [48, 48, SIZE - 48, SIZE - 48], radius=200, outline=(6, 78, 59, 255), width=16
)

# 字母 Q
font = ImageFont.truetype(
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf", 560
)
d.text((SIZE / 2, SIZE / 2), "Q", font=font, fill=(5, 46, 34, 255), anchor="mm")

img.save("/home/z/my-project/apps/shell-tauri/app-icon.png")
print("icon saved:", img.size)
