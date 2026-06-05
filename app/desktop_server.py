import os

import uvicorn

from app.main import app


def main() -> None:
    os.environ.setdefault("STOCK_PROVIDER", "astock")
    host = os.getenv("GP_ASSISTANT_HOST", "127.0.0.1")
    port = int(os.getenv("GP_ASSISTANT_PORT", "8010"))
    log_level = os.getenv("GP_ASSISTANT_LOG_LEVEL", "warning")
    uvicorn.run(app, host=host, port=port, log_level=log_level)


if __name__ == "__main__":
    main()
