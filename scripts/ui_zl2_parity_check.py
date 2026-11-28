#!/usr/bin/env python3
"""ZL2/FCL 对标功能层演示走查：登录 → 主页版本卡 → 下载页五分区 →
实例页 Tab + 实例设置 → 设置页 Java 管理 → 账户页，逐屏截图验收。

演示构建（VITE_DEMO=1）+ 模拟后端，验证新功能层在浏览器里全链路可用。
"""
import asyncio
import functools
import http.server
import pathlib
import sys
import threading

from playwright.async_api import async_playwright

DIST = pathlib.Path("/home/z/my-project/apps/shell-tauri/ui/dist-demo").resolve()
OUT = pathlib.Path("/tmp/render-ui")
if not OUT.exists():
    OUT.mkdir(parents=True)

PORT = 8932


def serve_dist() -> None:
    handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=str(DIST))
    server = http.server.ThreadingHTTPServer(("127.0.0.1", PORT), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()


async def main() -> None:
    serve_dist()
    async with async_playwright() as p:
        browser = await p.chromium.launch()
        page = await browser.new_page(viewport={"width": 393, "height": 851}, device_scale_factor=2)
        errors: list[str] = []
        page.on("pageerror", lambda e: errors.append(str(e)))
        await page.goto(f"http://127.0.0.1:{PORT}/index.html")
        await page.wait_for_timeout(1200)

        # 演示模式自动以内置账号进入主界面（mock get_account 常驻 Steve）
        await page.wait_for_timeout(800)
        await page.screenshot(path=str(OUT / "01-boot.png"))
        await page.wait_for_timeout(1400)
        await page.screenshot(path=str(OUT / "02-home.png"))

        async def go(page_id: str) -> None:
            await page.locator("#bottom-nav .nav-item", has_text=page_id).first.click()
            await page.wait_for_timeout(900)

        # 下载页：游戏分区
        await go("下载")
        await page.screenshot(path=str(OUT / "03-download-game.png"))
        # 模组分区（Modrinth 搜索 + 详情）
        await page.locator(".chip", has_text="模组").first.click()
        await page.wait_for_timeout(1200)
        await page.screenshot(path=str(OUT / "04-download-mods.png"))
        await page.get_by_text("版本列表", exact=False).first.click()
        await page.wait_for_timeout(900)
        await page.screenshot(path=str(OUT / "05-modrinth-detail.png"))
        # 一键安装（模拟进度）
        await page.get_by_text("一键安装", exact=False).first.click()
        await page.wait_for_timeout(1500)
        await page.screenshot(path=str(OUT / "06-modrinth-installing.png"))
        # 整合包分区（本地导入按钮 + Modrinth 整合包列表）
        await page.locator(".chip", has_text="整合包").first.click()
        await page.wait_for_timeout(1200)
        await page.screenshot(path=str(OUT / "07-download-modpack.png"))

        # 实例页：实例 Tab + 实例设置展开
        await go("实例")
        await page.wait_for_timeout(500)
        await page.screenshot(path=str(OUT / "08-instances.png"))
        await page.locator(".account-actions .btn", has_text="设置").first.click()
        await page.wait_for_timeout(600)
        await page.screenshot(path=str(OUT / "09-instance-settings.png"))
        # 模组内容分区（含禁用开关）
        await page.locator(".chip", has_text="模组").first.click()
        await page.wait_for_timeout(900)
        await page.screenshot(path=str(OUT / "10-content-mods.png"))

        # 设置页：Java 管理
        await go("设置")
        await page.wait_for_timeout(500)
        await page.locator("h3", has_text="Java 运行时管理").first.scroll_into_view_if_needed()
        await page.screenshot(path=str(OUT / "11-settings-java.png"))

        # 账户页
        await go("账户")
        await page.screenshot(path=str(OUT / "12-account.png"))

        await browser.close()
        print("shots:", len(list(OUT.glob("*.png"))))
        if errors:
            print("PAGE ERRORS:")
            for e in errors:
                print("  -", e)
            sys.exit(2)
        print("no page errors")


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
