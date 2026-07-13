#!/usr/bin/env python3
"""Demonstrate a solver's authorization and Custos pre-broadcast gates."""

import json
import sys
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen


API_BASE = "http://127.0.0.1:8787"
MERCHANT = "9xQeWvG816bUx9EPjHmaT23yvVMqZnjV8K3RkQQx9W7"

INTENT = {
    "amount_usdc": 5,
    "destination": MERCHANT,
}
POLICY = {
    "max_usdc": 100,
    "allowlist": {
        MERCHANT,
        "Vote111111111111111111111111111111111111111",
    },
}


def authorize(intent, policy):
    """Allow only an in-limit payment to a preapproved destination."""
    return (
        intent["amount_usdc"] <= policy["max_usdc"]
        and intent["destination"] in policy["allowlist"]
    )


def request_json(path, payload=None):
    if payload is None:
        request = Request(f"{API_BASE}{path}")
    else:
        request = Request(
            f"{API_BASE}{path}",
            data=json.dumps(payload).encode("utf-8"),
            headers={"Content-Type": "application/json"},
            method="POST",
        )

    try:
        with urlopen(request, timeout=10) as response:
            return json.load(response)
    except HTTPError as error:
        raise RuntimeError(f"Custos API request failed: HTTP {error.code}") from error
    except URLError as error:
        raise RuntimeError(
            "Custos API is not running. Start it with: cd api && cargo run"
        ) from error


def main():
    authorized = authorize(INTENT, POLICY)
    print(
        "Authorization policy (declared intent): "
        f"{'PASS' if authorized else 'FAIL'}"
    )

    build = request_json("/api/build")
    report = request_json(
        "/api/scan",
        {"tx_base64": build["tx_base64"], "user": build["owner"]},
    )
    if "error" in report:
        raise RuntimeError(f"Custos verification failed: {report['error']}")

    level = report["level"]
    print(f"Custos verification (actual tx): {level}")
    for finding in report.get("findings", []):
        print(
            f"  [{finding['code']}] {finding['account']} - {finding['message']}"
        )

    if authorized and level == "GREEN":
        print("Decision: BROADCAST")
    else:
        print("Decision: REFUSE TO BROADCAST - principal capital preserved")


if __name__ == "__main__":
    try:
        main()
    except (KeyError, RuntimeError, ValueError) as error:
        print(f"Error: {error}", file=sys.stderr)
        sys.exit(1)
