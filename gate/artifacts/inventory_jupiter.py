import json,urllib.request,time
RPC="https://api.mainnet-beta.solana.com"
def rpc(m,p):
    for _ in range(3):
        try:
            req=urllib.request.Request(RPC,json.dumps({"jsonrpc":"2.0","id":1,"method":m,"params":p}).encode(),{"Content-Type":"application/json"})
            return json.load(urllib.request.urlopen(req,timeout=25))
        except Exception as e:
            time.sleep(1)
    raise
JUP="JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4"
sigs=rpc("getSignaturesForAddress",[JUP,{"limit":30}])["result"]
ok=[s for s in sigs if not s.get("err")]
print("Jupiter v6 recent successful sigs:",len(ok))
picked=None
for s in ok[:15]:
    tx=rpc("getTransaction",[s["signature"],{"maxSupportedTransactionVersion":0,"encoding":"json"}])["result"]
    if not tx: continue
    ver=tx["transaction"].get("version","legacy")
    msg=tx["transaction"]["message"]
    static=msg["accountKeys"]
    loaded=tx["meta"].get("loadedAddresses",{}) or {}
    altw=loaded.get("writable",[]); altr=loaded.get("readonly",[])
    total=len(static)+len(altw)+len(altr)
    progs=set()
    maxdepth=0
    for l in tx["meta"].get("logMessages",[]):
        if l.startswith("Program ") and " invoke [" in l:
            progs.add(l.split()[1]); 
            d=int(l.split("[")[1].split("]")[0]); maxdepth=max(maxdepth,d)
    cu=tx["meta"].get("computeUnitsConsumed")
    nbal=len(tx["meta"].get("preTokenBalances",[]))
    print(f"  ver={ver} static={len(static)} alt={len(altw)+len(altr)} total={total} programs={len(progs)} max_cpi_depth={maxdepth} CU={cu} tokenbals={nbal}")
    if ver!="legacy" and picked is None and len(progs)>=3:
        picked=(s["signature"],tx,sorted(progs))
if picked:
    sig,tx,progs=picked
    json.dump({"sig":sig,"tx":tx},open(f"jup_{sig[:8]}.json","w"))
    print("\nPICKED:",sig[:12],"-> jup_%s.json"%sig[:8])
    print("distinct programs invoked:")
    for p in progs: print("   ",p)
