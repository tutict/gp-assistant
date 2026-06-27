from pathlib import Path

from dotenv import load_dotenv
from fastapi import FastAPI, HTTPException
from fastapi.responses import FileResponse, JSONResponse
from fastapi.staticfiles import StaticFiles

from app.api.routes import router as api_router

load_dotenv()

app = FastAPI(title="股选优", version="0.3.0")


@app.middleware("http")
async def no_cache_static_assets(request, call_next):
    response = await call_next(request)
    if (
        request.url.path == "/"
        or request.url.path.startswith("/assets/")
        or request.url.path.startswith("/vendor/")
        or request.url.path in {"/screening_rules.json", "/mobile-financial-snapshot.json", "/favicon.ico"}
    ):
        response.headers["Cache-Control"] = "no-store, max-age=0"
        response.headers["Pragma"] = "no-cache"
        response.headers["Expires"] = "0"
    return response


app.include_router(api_router, prefix="/api")

BASE_DIR = Path(__file__).resolve().parent
ROOT_DIR = BASE_DIR.parent
FRONTEND_DIST_DIR = ROOT_DIR / "desktop" / "mobile-dist"
FRONTEND_INDEX = FRONTEND_DIST_DIR / "index.html"

app.mount("/assets", StaticFiles(directory=FRONTEND_DIST_DIR / "assets", check_dir=False), name="frontend-assets")
app.mount("/vendor", StaticFiles(directory=FRONTEND_DIST_DIR / "vendor", check_dir=False), name="frontend-vendor")


@app.get("/")
def index():
    if not FRONTEND_INDEX.exists():
        return JSONResponse(
            status_code=503,
            content={
                "detail": "React frontend has not been built. Run `npm.cmd --prefix desktop/frontend run build` first.",
            },
        )
    return FileResponse(FRONTEND_INDEX)


@app.get("/health")
def health():
    return {"status": "ok"}


@app.get("/{asset_name}", include_in_schema=False)
def frontend_public_asset(asset_name: str):
    if "/" in asset_name or "\\" in asset_name:
        raise HTTPException(status_code=404, detail="Not found")
    asset_path = FRONTEND_DIST_DIR / asset_name
    if not asset_path.is_file():
        raise HTTPException(status_code=404, detail="Not found")
    return FileResponse(asset_path)
