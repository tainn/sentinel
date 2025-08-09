import json
import os
import time
from dataclasses import dataclass
from pathlib import Path

import httpx
from bs4 import BeautifulSoup
from cordhook import Form


@dataclass
class Struct:
    author_name: str
    author_url: str
    last_post_url: str
    last_post_id: str
    forum_name: str
    forum_url: str
    thread_name: str
    thread_url: str


def main() -> None:
    domain = os.getenv("DOMAIN")
    url = f"{domain}/search.php?search_id=active_topics"
    res = httpx.get(url, timeout=10)

    soup = BeautifulSoup(res.text, "html.parser")
    entries = soup.find_all("div", {"class": "list-inner"})

    persistence_path = "/data/persistence.json"

    if not Path(persistence_path).exists():
        with Path(persistence_path).open("w") as wf:
            wf.write("[]")

    with Path(persistence_path).open() as rf:
        persist = json.load(rf)

    for entry in entries:
        if entry.find("div", {"class": "responsive-show"}) is None:
            continue

        auth_lp_forum = entry.find("div", {"class": "responsive-show"})
        auth_lp_forum_data = auth_lp_forum.find_all("a")
        thread = entry.find("a", {"class": "topictitle"})

        struct = Struct(
            author_name=auth_lp_forum_data[0].text,
            author_url=parse_url(auth_lp_forum_data[0]["href"]),
            last_post_url=parse_url(auth_lp_forum_data[1]["href"], post=True),
            last_post_id=parse_url(auth_lp_forum_data[1]["href"], post=True).split("#")[-1],
            forum_name=auth_lp_forum_data[2].text,
            forum_url=parse_url(auth_lp_forum_data[2]["href"]),
            thread_name=thread.text,
            thread_url=parse_url(thread["href"]),
        )

        if struct.last_post_id in persist:
            print(f"post previously already collected: {struct.last_post_id}")
            continue

        print(f"new post collected: {struct.last_post_id}")

        persist.append(struct.last_post_id)

        if len(persist) > int(os.getenv("PERSIST_QUANTITY")):
            persist.pop(0)

        with Path(persistence_path).open("w") as wf:
            json.dump(persist, wf, indent=4)

        discord_webhook(struct)


def parse_url(raw_url: str, post: bool = False) -> str:
    clean_url = raw_url.split("sid=")[0].replace("amp;", "").strip("&")

    if post:
        post_id = clean_url.split("=")[-1]
        clean_url = f"{clean_url}#p{post_id}"

    return f"{os.getenv('DOMAIN')}/{clean_url[2:]}"


def discord_webhook(ec: Struct) -> None:
    description = (
        f"new [**post**]({ec.last_post_url}) "
        f"by [{ec.author_name}]({ec.author_url}) "
        f"in [{ec.thread_name}]({ec.thread_url})"
    )

    form = Form()

    form.username(os.getenv("WEBHOOK_USERNAME"))
    form.avatar_url(os.getenv("WEBHOOK_AVATAR"))
    form.embed_color(0000000)
    form.embed_description(description)

    webhook_channels = os.getenv("WEBHOOK_CHANNELS")
    assert webhook_channels

    for hook in json.loads(webhook_channels):
        form.post(hook)


if __name__ == "__main__":
    print("running sentinel...")

    while True:
        try:
            main()
            time.sleep(float(os.getenv("MONITOR_INTERVAL")))

        except Exception as e:
            print(f"global exception caught: {e}")
            time.sleep(float(os.getenv("ERR_RETRY_INTERVAL")))
