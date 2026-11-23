#!/usr/bin/env python3
"""竖屏布局视觉验证：393x851（主流安卓竖屏 CSS 视口）渲染 UI 首屏截图。

登录屏 + 主屏（注入账号跳过登录）两个状态各截一张，
再截一张 704 宽（用户截图同宽）确认无挤压。
"""
import asyncio
import functools
import http.server
import pathlib
import sys
import threading

from playwright.async_api import async_playwright

DIST = pathlib.Path("/home/z/my-project/apps/shell-tauri/ui/dist").resolve()
OUT = pathlib.Path("/tmp/render-ui")

if not OUT.exists():
    OUT.mkdir(parents=True)

PORT = 8931


def serve_dist() -> None:
    handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=str(DIST))
    server = http.server.ThreadingHTTPServer(("127.0.0.1", PORT), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()


async def main() -> None:
    serve_dist()
    async with async_playwright() as p:
        browser = await p.chromium.launch()
        page = await browser.new_page(viewport={"width": 393, "height": 851}, device_scale_factor=2)
        await page.goto(f"http://127.0.0.1:{PORT}/index.html")
        await page.wait_for_timeout(1200)
        # 状态1：登录屏
        await page.screenshot(path=str(OUT / "portrait-login.png"))
        # 跳过登录直接看主屏（DOM 注入以检查布局）
        await page.evaluate(
            """() => {
              const login = document.getElementById('login-screen');
              const main = document.getElementById('main-screen');
              if (login) login.classList.add('hidden');
              if (main) main.classList.remove('hidden');
            }"""
        )
        await page.wait_for_timeout(600)
        await page.screenshot(path=str(OUT / "portrait-main.png"))
        # 状态2：宽 704（对齐用户截图的设备宽度）确认不再出现竖排挤压
        await page.set_viewport_size({"width": 704, "height": 1568})
        await page.wait_for_timeout(400)
        await page.screenshot(path=str(OUT / "portrait-704.png"))
        await browser.close()
    print("done:", OUT)


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
