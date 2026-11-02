#!/usr/bin/env python3
"""按配置生成 / 更新 README 顶部的徽章区块。

生成的区块包含两部分（全部居中）：
  1. 顶部徽章：协议 / 平台 / 作者 / 状态 / AI 辅助生成，以及按需的动态徽章
  2. 标题下方的语言切换徽章 + 文档导航

只改写下面两对标记之间的内容，其余正文保持原样：
    <!-- badges:start --> ... <!-- badges:end -->
    <!-- language-nav:start --> ... <!-- language-nav:end -->

用法：
    python scripts/update_badges.py                # 就地更新
    python scripts/update_badges.py --check        # 只检查，过期则退出码 1
    python scripts/update_badges.py --print        # 打印生成结果，不写文件
    python scripts/update_badges.py --config X.json --readme Y.md
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path
from urllib.parse import quote

START_MARKER = "<!-- badges:start -->"
END_MARKER = "<!-- badges:end -->"
LANGUAGE_START_MARKER = "<!-- language-nav:start -->"
LANGUAGE_END_MARKER = "<!-- language-nav:end -->"

CURRENT_COLOR = "2ea44f"
AVAILABLE_COLOR = "blue"
MISSING_COLOR = "lightgrey"

DYNAMIC_BADGES = {
    "release": ("Release", "github/v/release", "releases"),
    "stars": ("Stars", "github/stars", "stargazers"),
    "issues": ("Issues", "github/issues", "issues"),
    "forks": ("Forks", "github/forks", "network/members"),
    "prs": ("Pull Requests", "github/issues-pr", "pulls"),
}

REQUIRED_FIELDS = ("owner", "repo", "project", "license", "readme")


def escape_badge_text(text: str) -> str:
    """shields.io 静态徽章的文本转义规则。"""
    return str(text).replace("-", "--").replace("_", "__").replace(" ", "_")


def badge_source(label: str, message: str, color: str, style: str,
                 logo: str | None = None, logo_color: str | None = None) -> str:
    label_text = quote(escape_badge_text(label), safe="")
    message_text = quote(escape_badge_text(message), safe="")
    color_text = quote(str(color), safe="")
    source = f"https://img.shields.io/badge/{label_text}-{message_text}-{color_text}"
    params = [f"style={style}"]
    if logo:
        params.append(f"logo={logo}")
    if logo_color:
        params.append(f"logoColor={logo_color}")
    return source + "?" + "&".join(params)


def render_badge(label: str, message: str, color: str, href: str | None,
                 style: str, logo: str | None = None,
                 logo_color: str | None = None) -> str:
    source = badge_source(label, message, color, style, logo, logo_color)
    alt = escape_html(label if message == "" else f"{label} {message}")
    image = f'<img alt="{alt}" src="{source}">'
    if href:
        return f'<a href="{escape_html(href)}">{image}</a>'
    return image


def escape_html(text: str) -> str:
    return (str(text).replace("&", "&amp;")
            .replace('"', "&quot;")
            .replace("<", "&lt;")
            .replace(">", "&gt;"))


def relative_href(target: Path, start_dir: Path) -> str:
    relative = os.path.relpath(target, start_dir)
    return Path(relative).as_posix()


def load_config(path: Path) -> dict:
    if not path.is_file():
        raise SystemExit(f"配置文件不存在: {path}")
    try:
        config = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        raise SystemExit(f"配置文件不是合法 JSON: {path}\n{error}")
    missing = [field for field in REQUIRED_FIELDS if not config.get(field)]
    if missing:
        raise SystemExit(f"配置缺少必填字段: {', '.join(missing)}")
    return config


def render_core_badges(config: dict, readme_dir: Path, repo_root: Path) -> list[str]:
    style = config.get("badge_style", "flat")
    owner = config["owner"]
    repo = config["repo"]
    repo_url = f"https://github.com/{owner}/{repo}"
    license_file = repo_root / "LICENSE"
    license_href = relative_href(license_file, readme_dir) if license_file.is_file() else repo_url

    badges = [
        render_badge("协议", config["license"], "blue", license_href, style),
    ]

    if config.get("badge_profile") == "data":
        badges.append(render_badge(
            "类型",
            config.get("dataset_type", "数据集"),
            "blue",
            repo_url,
            style,
        ))
        badges.append(render_badge(
            "更新",
            config.get("update_frequency", "不定期"),
            "green",
            repo_url,
            style,
        ))
    else:
        badges.append(render_badge(
            "平台",
            config.get("platform", "跨平台"),
            "black",
            repo_url,
            style,
            logo=config.get("platform_logo"),
            logo_color=config.get("platform_logo_color", "white"),
        ))

    badges.append(render_badge("作者", owner, "orange", f"https://github.com/{owner}", style))

    if config.get("badge_profile") != "data":
        if config.get("status"):
            badges.append(render_badge("状态", config["status"], "green", repo_url, style))

        if config.get("ai_assisted"):
            badges.append(render_badge(
                "AI辅助生成",
                config.get("ai_message", "是"),
                "orange",
                repo_url,
                style,
            ))

    return badges


def render_dynamic_badges(config: dict) -> list[str]:
    style = config.get("badge_style", "flat")
    owner = config["owner"]
    repo = config["repo"]
    repo_url = f"https://github.com/{owner}/{repo}"
    badges = []
    for name in config.get("dynamic_badges", []):
        if name not in DYNAMIC_BADGES:
            raise SystemExit(
                f"未知动态徽章: {name}（可选: {', '.join(DYNAMIC_BADGES)}）"
            )
        label, endpoint, suffix = DYNAMIC_BADGES[name]
        source = f"https://img.shields.io/{endpoint}/{owner}/{repo}?style={style}"
        href = f"{repo_url}/{suffix}" if suffix else repo_url
        badges.append(
            f'<a href="{escape_html(href)}"><img alt="{escape_html(label)}" src="{source}"></a>'
        )
    return badges


def render_language_badges(config: dict, readme_dir: Path,
                           repo_root: Path) -> tuple[list[str], list[str]]:
    style = config.get("badge_style", "flat")
    badges: list[str] = []
    missing: list[str] = []
    languages = config.get("languages", [])

    if languages and sum(1 for item in languages if item.get("current")) != 1:
        raise SystemExit("配置 languages 中必须且只能有一项 current=true")

    for language in languages:
        name = language.get("name") or language.get("file")
        if not name:
            raise SystemExit("languages 条目缺少 name / file")
        file_value = language.get("file")
        if not file_value:
            raise SystemExit(f"语言 {name} 缺少 file")
        target = (repo_root / file_value).resolve()

        if language.get("current"):
            message, color = language.get("current_message", "当前"), CURRENT_COLOR
            href = relative_href(target, readme_dir) if target.is_file() else None
        elif target.is_file():
            message = language.get("message", "切换")
            color = AVAILABLE_COLOR
            href = relative_href(target, readme_dir)
        else:
            message, color, href = language.get("missing_message", "待补"), MISSING_COLOR, None
            missing.append(file_value)

        badges.append(render_badge(name, message, color, href, style))

    return badges, missing


def render_docs_nav(config: dict) -> str | None:
    docs = config.get("docs", [])
    if not docs:
        return None
    links = [
        f'<a href="{escape_html(item["href"])}">{escape_html(item["label"])}</a>'
        for item in docs
        if item.get("label") and item.get("href")
    ]
    if not links:
        return None
    return '<sub>文档 / Docs：' + " · ".join(links) + "</sub>"


def wrap_center(groups: list[list[str]]) -> list[str]:
    parts: list[str] = []
    for index, group in enumerate(groups):
        if index:
            parts.append("")
        parts.append('<p align="center">')
        parts.extend("  " + item for item in group)
        parts.append("</p>")
    return parts


def render_blocks(config: dict) -> tuple[str, str, list[str]]:
    repo_root = Path(config["_repo_root"]).resolve()
    readme_path = (repo_root / config["readme"]).resolve()
    readme_dir = readme_path.parent

    badge_groups: list[list[str]] = []
    core = render_core_badges(config, readme_dir, repo_root)
    if core:
        badge_groups.append(core)
    dynamic = render_dynamic_badges(config)
    if dynamic:
        badge_groups.append(dynamic)

    language_groups: list[list[str]] = []
    language_badges, missing_languages = render_language_badges(config, readme_dir, repo_root)
    if language_badges:
        language_groups.append(language_badges)
    docs_nav = render_docs_nav(config)
    if docs_nav:
        language_groups.append([docs_nav])

    badge_block = "\n".join([START_MARKER, *wrap_center(badge_groups), END_MARKER]) + "\n"
    language_block = "\n".join(
        [LANGUAGE_START_MARKER, *wrap_center(language_groups), LANGUAGE_END_MARKER]
    ) + "\n"
    return badge_block, language_block, missing_languages


def replace_block(text: str, block: str, start_marker: str, end_marker: str,
                  readme_path: Path) -> str:
    start = text.find(start_marker)
    end = text.find(end_marker, start + len(start_marker)) if start != -1 else -1
    if start == -1 or end == -1:
        raise SystemExit(
            f"README 缺少标记，请先加入：\n"
            f"  {start_marker}\n  {end_marker}\n"
            f"文件: {readme_path}"
        )
    tail = text[end + len(end_marker):].lstrip("\n")
    return text[:start] + block + ("\n" + tail if tail else "")


def main() -> int:
    parser = argparse.ArgumentParser(description="生成 / 更新 README 顶部徽章区块")
    parser.add_argument("--config", help="配置文件路径，默认 <repo>/scripts/badges.config.json")
    parser.add_argument("--readme", help="README 路径，默认取配置中的 readme")
    parser.add_argument("--repo-root", help="仓库根目录，默认脚本上一级")
    parser.add_argument("--check", action="store_true", help="只检查，不写文件；过期返回 1")
    parser.add_argument("--print", dest="print_only", action="store_true", help="打印生成结果")
    args = parser.parse_args()

    repo_root = Path(args.repo_root).resolve() if args.repo_root else Path(__file__).resolve().parents[1]
    config_path = Path(args.config).resolve() if args.config else repo_root / "scripts" / "badges.config.json"
    config = load_config(config_path)
    config["_repo_root"] = str(repo_root)
    if args.readme:
        config["readme"] = args.readme

    readme_path = (repo_root / config["readme"]).resolve()
    if not readme_path.is_file():
        raise SystemExit(f"README 不存在: {readme_path}")

    badge_block, language_block, missing_languages = render_blocks(config)
    original = readme_path.read_text(encoding="utf-8")
    updated = replace_block(original, badge_block, START_MARKER, END_MARKER, readme_path)
    updated = replace_block(
        updated, language_block, LANGUAGE_START_MARKER, LANGUAGE_END_MARKER, readme_path
    )

    if args.print_only:
        sys.stdout.write(badge_block + "\n" + language_block)
        return 0

    if args.check:
        if updated != original:
            print(f"[outdated] 徽章区块需要更新: {readme_path}")
            print("运行 python scripts/update_badges.py 后提交。")
            return 1
        print(f"[ok] 徽章区块已是最新: {readme_path}")
    else:
        if updated != original:
            readme_path.write_text(updated, encoding="utf-8", newline="\n")
            print(f"[updated] {readme_path}")
        else:
            print(f"[ok] 无需改动: {readme_path}")

    for file_value in missing_languages:
        print(f"[warn] 语言文件不存在，徽章已降级为「待补」: {file_value}")
    print("[hint] 当前语言徽章、文档导航均可在 scripts/badges.config.json 中调整。")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
