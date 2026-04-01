# rssbot [![Build Status](https://github.com/iovxw/rssbot/workflows/Rust/badge.svg)](https://github.com/iovxw/rssbot/actions?query=workflow%3ARust) [![Github All Releases](https://img.shields.io/github/downloads/iovxw/rssbot/total.svg)](https://github.com/iovxw/rssbot/releases)

**Other Languages:** [English](README.en.md)

**支持:**
 - [x] RSS 0.9
 - [x] RSS 0.91
 - [x] RSS 0.92
 - [x] RSS 0.93
 - [x] RSS 0.94
 - [x] RSS 1.0
 - [x] RSS 2.0
 - [x] Atom 0.3
 - [x] Atom 1.0
 - [x] JSON Feed 1

## 使用

    /rss       - 显示当前订阅的 RSS 列表
    /sub       - 订阅一个 RSS: /sub http://example.com/feed.xml
    /unsub     - 退订一个 RSS: /unsub http://example.com/feed.xml
    /export    - 导出为 OPML

## 下载

可直接从 [Releases](https://github.com/iovxw/rssbot/releases) 下载预编译的程序（带 `zh` 的为中文版）, Linux 版本为 *musl* 静态链接, 无需其他依赖

## 编译

**请先尝试从上面下载, 如不可行或者有其他需求再手动编译**

先安装 *Rust* 以及 *Cargo* (推荐使用 [`rustup`](https://www.rustup.rs/)), 然后:

```
cargo build --release
```

编译好的文件位于: `./target/release/rssbot`

## 运行

```
USAGE:
    rssbot [FLAGS] [OPTIONS] <token>

FLAGS:
        --fetch-on-start    启动时立即拉取所有订阅，而不是等待第一个抓取周期
    -h, --help              打印帮助信息
        --insecure          危险: 不安全模式，接受无效的 TLS 证书
        --restricted        仅允许群组管理员使用 Bot 命令
        --systemd           使用 systemd 兼容的日志格式（无时间戳，仅输出级别前缀）
    -v, --verbose           启用详细日志 (-v: fetcher 日志; -vv: 同时显示 rustls/reqwest/h2; -vvv: 同时显示 teloxide)
    -V, --version           打印版本信息

OPTIONS:
        --admin <user id>...        私有模式，仅允许指定用户使用此 Bot，可多次传入以允许多个管理员
        --api-uri <tgapi-uri>       自定义 Telegram API 地址 [默认: https://api.telegram.org/]
    -d, --database <path>           数据库路径 [默认: ./rssbot.json]
        --max-feed-size <bytes>     RSS 最大体积，0 为不限制 [默认: 2M]
        --max-interval <seconds>    最大抓取间隔（秒）[默认: 43200]
        --min-interval <seconds>    最小抓取间隔（秒）[默认: 300]

ARGS:
    <token>    Telegram Bot Token

NOTE: 可通过 @userinfobot @getidsbot 等机器人获取 <user id>
```

`<token>` 请参照 [这里](https://core.telegram.org/bots#3-how-do-i-create-a-bot) 申请

## 环境变量

- `RUST_LOG`: 日志过滤器，例如 `RUST_LOG=rssbot=debug`（参见 [env_logger 文档](https://docs.rs/env_logger/)）
- `HTTP_PROXY`: 用于 HTTP 的代理
- `HTTPS_PROXY`: 用于 HTTPS 的代理
- `RSSBOT_DONT_PROXY_FEEDS`: 设为 `1` 使所有订阅的 RSS 不通过代理（仅代理 Telegram）
- `NO_PROXY`: 暂不支持，等待 [reqwest#877](https://github.com/seanmonstar/reqwest/pull/877)

## 从旧的 RSSBot 迁移

对于 [原先 Clojure 版本的 Bot](https://github.com/iovxw/tg-rss-bot), 可以使用以下脚本转换数据库

```bash
#!/bin/bash

DATABASE=$1
TARGET=$2

DATA=$(echo "SELECT url, title FROM rss;" | sqlite3 $DATABASE)
IFS=$'\n'

echo -e "[\c" > $TARGET
for line in ${DATA[@]}
do
    IFS='|'
    r=($line)
    link=${r[0]}
    title=${r[1]}

    echo -e "{\"link\":\"$link\"," \
            "\"title\":\"$title\"," \
            "\"error_count\":0," \
            "\"hash_list\":[]," \
            "\"subscribers\":[\c" >> $TARGET

    subscribers=$(echo "SELECT subscriber FROM subscribers WHERE rss='$link';" | sqlite3 $DATABASE)
    IFS=$'\n'
    for subscriber in ${subscribers[@]}
    do
        echo -e "$subscriber,\c" >> $TARGET
    done

    echo -e "]},\c" >> $TARGET
done
echo "]" >> $TARGET
sed -i "s/,]/]/g" $TARGET
```

参数 1 为旧数据库地址, 2 为结果输出地址

需要注意的是已推送的 RSS 记录不会保留, 如果直接使用转换后的数据库, 会重复推送旧的 RSS

## License

This is free and unencumbered software released into the public domain.

Anyone is free to copy, modify, publish, use, compile, sell, or distribute this software, either in source code form or as a compiled binary, for any purpose, commercial or non-commercial, and by any means.
