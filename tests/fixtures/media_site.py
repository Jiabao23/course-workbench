"""Loopback-only media fixture for real yt-dlp/desktop integration checks."""
import argparse
import functools
import json
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlsplit


class MediaSite(SimpleHTTPRequestHandler):
    request_log: Path

    def log_message(self, fmt, *args):
        with self.request_log.open("a", encoding="utf-8") as log:
            log.write(json.dumps({"method": self.command, "path": self.path,
                                  "range": self.headers.get("Range"),
                                  "message": fmt % args}, ensure_ascii=False) + "\n")

    def do_GET(self):
        path = urlsplit(self.path).path
        if path == "/unavailable":
            self.send_error(503, "Temporary fixture failure")
            return
        if path in ("/with-subtitles", "/without-subtitles"):
            subtitle = '<track kind="subtitles" src="/lesson.vtt" srclang="zh" label="中文">' if path == "/with-subtitles" else ""
            title = "网页字幕优先验证" if subtitle else "网页音频回退验证"
            body = (f'<!doctype html><html><head><meta charset="utf-8"><title>{title}</title></head>'
                    f'<body><h1>{title}</h1><video controls><source src="/sample.mp4" type="video/mp4">'
                    f'{subtitle}</video></body></html>').encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        super().do_GET()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--log", type=Path, required=True)
    parser.add_argument("--port", type=int, default=18444)
    args = parser.parse_args()
    MediaSite.request_log = args.log.resolve()
    MediaSite.request_log.parent.mkdir(parents=True, exist_ok=True)
    handler = functools.partial(MediaSite, directory=str(args.directory.resolve()))
    server = ThreadingHTTPServer(("127.0.0.1", args.port), handler)
    print(json.dumps({"url": f"http://127.0.0.1:{server.server_port}"}), flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
