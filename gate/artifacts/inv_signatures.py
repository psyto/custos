import json,urllib.request
RPC="https://api.mainnet-beta.solana.com"
def rpc(m,p):
    req=urllib.request.Request(RPC,json.dumps({"jsonrpc":"2.0","id":1,"method":m,"params":p}).encode(),{"Content-Type":"application/json"})
    return json.load(urllib.request.urlopen(req,timeout=20))
RAY="675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8"
sigs=rpc("getSignaturesForAddress",[RAY,{"limit":25}])["result"]
ok=[s for s in sigs if not s.get("err")]
print("recent successful sigs:",len(ok))
for s in ok[:12]:
    tx=rpc("getTransaction",[s["signature"],{"maxSupportedTransactionVersion":0,"encoding":"jsonParsed"}])["result"]
    if not tx: continue
    ver=tx["transaction"].get("version","legacy")
    msg=tx["transaction"]["message"]
    static=[a["pubkey"] for a in msg["accountKeys"]]
    loaded=tx["meta"].get("loadedAddresses",{}) or {}
    alt=len(loaded.get("writable",[]))+len(loaded.get("readonly",[]))
    total=len(static)+alt
    # distinct programs invoked (from logs)
    progs=set()
    for l in tx["meta"].get("logMessages",[]):
        if l.startswith("Program ") and " invoke [" in l:
            progs.add(l.split()[1])
    cu=tx["meta"].get("computeUnitsConsumed")
    print(f"  ver={ver} static_accts={len(static)} alt_accts={alt} total={total} distinct_programs={len(progs)} CU={cu}")
    # keep the first legacy one for the gate
    if ver=="legacy" and s is ok[0] or ver=="legacy":
        json.dump({"sig":s["signature"],"tx":tx},open(f"ray_{s['signature'][:8]}.json","w"))
        print(f"    saved legacy candidate: ray_{s['signature'][:8]}.json  programs={sorted(progs)}")
