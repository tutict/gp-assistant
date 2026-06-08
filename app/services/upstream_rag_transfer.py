from __future__ import annotations

import json
import mimetypes
import secrets
import socket
import threading
from dataclasses import dataclass
from datetime import datetime, timedelta
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, quote, urlparse

from app.services.upstream_rag_pack import latest_upstream_pack


@dataclass
class TransferSession:
    token: str
    expires_at: datetime
    host: str
    port: int
    manifest_path: Path
    pack_path: Path
    server: ThreadingHTTPServer
    thread: threading.Thread


_active_session: TransferSession | None = None
_session_lock = threading.Lock()


def start_upstream_rag_transfer(ttl_minutes: int = 15) -> dict:
    manifest = latest_upstream_pack()
    if not manifest:
        raise ValueError("尚未构建上下游 RAG 包。")
    if not manifest.get("valid"):
        raise ValueError("当前上下游 RAG 包未通过质量门禁，不能同步到手机。")

    manifest_path = Path(manifest["_manifest_path"])
    pack_path = Path(manifest["_pack_path"])
    if not manifest_path.exists() or not pack_path.exists():
        raise ValueError("上下游 RAG 包文件不存在，请重新构建。")

    with _session_lock:
        _stop_active_locked()
        token = secrets.token_urlsafe(18)
        expires_at = datetime.now() + timedelta(minutes=max(1, min(ttl_minutes, 60)))
        state = {
            "token": token,
            "expires_at": expires_at,
            "manifest_path": manifest_path,
            "pack_path": pack_path,
        }
        server = ThreadingHTTPServer(("0.0.0.0", 0), _handler_for_state(state))
        host = _lan_ip()
        port = int(server.server_address[1])
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        global _active_session
        _active_session = TransferSession(
            token=token,
            expires_at=expires_at,
            host=host,
            port=port,
            manifest_path=manifest_path,
            pack_path=pack_path,
            server=server,
            thread=thread,
        )

    manifest_url = f"http://{host}:{port}/manifest.json?token={quote(token)}"
    pack_url = f"http://{host}:{port}/rag_pack.sqlite?token={quote(token)}"
    import_payload = {
        "manifest_url": manifest_url,
        "pack_url": pack_url,
        "token": token,
        "expires_at": expires_at.isoformat(timespec="seconds"),
    }
    return {
        **import_payload,
        "qr_svg": _qr_svg_data_uri(json.dumps(import_payload, ensure_ascii=False, separators=(",", ":"))),
        "notes": [
            "请确保手机和桌面端在同一局域网。",
            "下载链接只在过期时间前有效，且只暴露当前上下游 RAG 包。",
        ],
    }


def active_upstream_rag_transfer() -> dict:
    with _session_lock:
        session = _active_session
        if session is None:
            return {"active": False}
        if datetime.now() >= session.expires_at:
            _stop_active_locked()
            return {"active": False}
        return {
            "active": True,
            "manifest_url": f"http://{session.host}:{session.port}/manifest.json?token={quote(session.token)}",
            "pack_url": f"http://{session.host}:{session.port}/rag_pack.sqlite?token={quote(session.token)}",
            "expires_at": session.expires_at.isoformat(timespec="seconds"),
        }


def stop_upstream_rag_transfer() -> None:
    with _session_lock:
        _stop_active_locked()


def _stop_active_locked() -> None:
    global _active_session
    if _active_session is not None:
        _active_session.server.shutdown()
        _active_session.server.server_close()
    _active_session = None


def _handler_for_state(state: dict):
    class UpstreamRagTransferHandler(BaseHTTPRequestHandler):
        def do_GET(self):
            parsed = urlparse(self.path)
            token = parse_qs(parsed.query).get("token", [""])[0]
            if token != state["token"] or datetime.now() >= state["expires_at"]:
                self.send_error(HTTPStatus.FORBIDDEN, "invalid or expired token")
                return
            if parsed.path == "/manifest.json":
                self._send_file(state["manifest_path"], "application/json; charset=utf-8")
                return
            if parsed.path == "/rag_pack.sqlite":
                self._send_file(state["pack_path"], "application/octet-stream")
                return
            self.send_error(HTTPStatus.NOT_FOUND, "not found")

        def log_message(self, format, *args):  # noqa: A003
            return

        def _send_file(self, path: Path, content_type: str | None = None) -> None:
            if not path.exists():
                self.send_error(HTTPStatus.NOT_FOUND, "file not found")
                return
            self.send_response(HTTPStatus.OK)
            self.send_header("Content-Type", content_type or mimetypes.guess_type(path.name)[0] or "application/octet-stream")
            self.send_header("Content-Length", str(path.stat().st_size))
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            with path.open("rb") as handle:
                while True:
                    block = handle.read(1024 * 1024)
                    if not block:
                        break
                    self.wfile.write(block)

    return UpstreamRagTransferHandler


def _lan_ip() -> str:
    try:
        sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        try:
            sock.connect(("223.5.5.5", 80))
            return sock.getsockname()[0]
        finally:
            sock.close()
    except OSError:
        try:
            return socket.gethostbyname(socket.gethostname())
        except OSError:
            return "127.0.0.1"


def _qr_svg_data_uri(payload: str) -> str | None:
    try:
        import qrcode
        import qrcode.image.svg
    except ImportError:
        return None
    factory = qrcode.image.svg.SvgPathImage
    image = qrcode.make(payload, image_factory=factory, box_size=10, border=2)
    raw = image.to_string(encoding="unicode")
    return f"data:image/svg+xml;utf8,{quote(raw)}"
