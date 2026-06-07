import argparse
import json
import os
import random
from datetime import datetime, timedelta
from typing import List, Tuple

import pandas as pd

from scripts.rqdata_http_client import RQDataHttpClient


def parse_args():
    parser = argparse.ArgumentParser(description="Build training datasets from RQData HTTP API.")
    parser.add_argument("--start-date", default="20180101", help="YYYYMMDD")
    parser.add_argument("--end-date", default=datetime.now().strftime("%Y%m%d"), help="YYYYMMDD")
    parser.add_argument("--intent-samples", type=int, default=1000)
    parser.add_argument("--report-samples", type=int, default=100)
    parser.add_argument("--out-dir", default="data/datasets")
    parser.add_argument("--seed", type=int, default=42)
    parser.add_argument("--instrument-cache", default="data/cache/instruments.csv")
    parser.add_argument("--pool-size", type=int, default=80)
    return parser.parse_args()


def fetch_instruments(client: RQDataHttpClient, cache_path: str) -> pd.DataFrame:
    if os.path.exists(cache_path):
        return pd.read_csv(cache_path)
    df = client.call("all_instruments", type="CS", market="cn")
    os.makedirs(os.path.dirname(cache_path), exist_ok=True)
    df.to_csv(cache_path, index=False)
    return df


def pick_col(df: pd.DataFrame, candidates: List[str]) -> str:
    for col in candidates:
        if col in df.columns:
            return col
    raise KeyError(f"Missing column, expected one of {candidates}")


def build_intent_samples(industries: List[str], count: int, date_range: Tuple[str, str]) -> List[dict]:
    start_date, end_date = date_range
    samples: List[dict] = []
    for _ in range(count):
        industry = random.choice(industries) if industries else None
        pe = round(random.uniform(5, 20), 1)
        pb = round(random.uniform(0.5, 5), 2)
        roe = round(random.uniform(5, 20), 1)
        top_n = random.choice([5, 10, 15, 20])
        if random.random() < 0.5:
            instruction = f"帮我筛选PE低于{pe}、ROE大于{roe}%的{industry or 'A股'}"
            output = {
                "action": "screen",
                "criteria": {"max_pe": pe, "min_roe": roe / 100, "industry": industry},
            }
        else:
            instruction = f"回测{start_date}到{end_date}，条件是PE<{pe}、PB<{pb}，Top {top_n}"
            output = {
                "action": "backtest",
                "criteria": {"max_pe": pe, "max_pb": pb},
                "start_date": start_date,
                "end_date": end_date,
                "top_n": top_n,
            }
        samples.append({"instruction": instruction, "input": "", "output": output})
    return samples


def fetch_price_pool(client: RQDataHttpClient, codes: List[str], start_date: str, end_date: str) -> pd.DataFrame:
    df = client.call(
        "get_price",
        order_book_ids=codes,
        start_date=start_date,
        end_date=end_date,
        fields=["close"],
        adjust_type="none",
    )
    return df


def build_report_samples(price_df: pd.DataFrame, codes: List[str], count: int, start_date: str, end_date: str) -> List[dict]:
    if price_df.empty:
        return []
    date_col = pick_col(price_df, ["date", "datetime", "trading_date"])
    code_col = pick_col(price_df, ["order_book_id", "code", "symbol"])
    close_col = pick_col(price_df, ["close", "close_price"])

    df = price_df[[date_col, code_col, close_col]].copy()
    df[date_col] = pd.to_datetime(df[date_col])
    pivot = df.pivot(index=date_col, columns=code_col, values=close_col).sort_index()
    pivot = pivot.ffill().dropna(how="all")

    samples: List[dict] = []
    for _ in range(count):
        top_n = random.choice([5, 10, 15, 20])
        picked = random.sample(codes, k=min(top_n, len(codes)))
        sub = pivot[picked].dropna(how="all")
        if len(sub) < 2:
            continue
        normalized = sub / sub.iloc[0]
        portfolio = normalized.mean(axis=1)
        total_return = float(portfolio.iloc[-1] / portfolio.iloc[0] - 1)
        rolling_max = portfolio.cummax()
        max_drawdown = float((portfolio / rolling_max - 1).min())
        days = max((portfolio.index[-1] - portfolio.index[0]).days, 1)
        annualized = float((1 + total_return) ** (365 / days) - 1)

        report = (
            f"本次回测区间为{start_date}-{end_date}，"
            f"等权组合总收益{total_return:.2%}，年化{annualized:.2%}，"
            f"最大回撤{max_drawdown:.2%}。"
        )
        samples.append(
            {
                "instruction": "根据回测结果生成简短中文报告",
                "input": {
                    "metrics": {
                        "total_return": total_return,
                        "annualized_return": annualized,
                        "max_drawdown": max_drawdown,
                    },
                    "symbols": picked,
                    "period": f"{start_date}-{end_date}",
                },
                "output": report,
            }
        )
    return samples


def write_jsonl(path: str, rows: List[dict]) -> None:
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        for row in rows:
            f.write(json.dumps(row, ensure_ascii=False) + "\n")


def main():
    args = parse_args()
    random.seed(args.seed)

    client = RQDataHttpClient()
    instruments = fetch_instruments(client, args.instrument_cache)
    industry_col = pick_col(instruments, ["industry_name", "sector_code_name", "industry"])
    code_col = pick_col(instruments, ["order_book_id", "code", "symbol"])
    industries = [x for x in instruments[industry_col].dropna().unique().tolist() if x]

    intent_samples = build_intent_samples(industries, args.intent_samples, (args.start_date, args.end_date))
    write_jsonl(os.path.join(args.out_dir, "intent.jsonl"), intent_samples)

    pool = instruments[code_col].dropna().unique().tolist()[: args.pool_size]
    price_df = fetch_price_pool(client, pool, args.start_date, args.end_date)
    report_samples = build_report_samples(price_df, pool, args.report_samples, args.start_date, args.end_date)
    write_jsonl(os.path.join(args.out_dir, "report.jsonl"), report_samples)

    print(f"intent.jsonl: {len(intent_samples)} samples")
    print(f"report.jsonl: {len(report_samples)} samples")


if __name__ == "__main__":
    main()
