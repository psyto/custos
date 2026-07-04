import json,urllib.request,time,subprocess,os
RPC="https://api.mainnet-beta.solana.com"
def rpc(m,p):
    last=None
    for _ in range(6):
        try:
            req=urllib.request.Request(RPC,json.dumps({"jsonrpc":"2.0","id":1,"method":m,"params":p}).encode(),{"Content-Type":"application/json"})
            r=json.load(urllib.request.urlopen(req,timeout=30))
            if "result" in r: return r["result"]
            last=r
        except Exception as e: last=e
        time.sleep(2)
    raise RuntimeError(f"rpc {m}: {last}")
JUP="JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4"   # the critique's named case
sigs=[s for s in rpc("getSignaturesForAddress",[JUP,{"limit":12}]) if not s.get("err")]
BUILTIN={"11111111111111111111111111111111","ComputeBudget111111111111111111111111111111",
 "BPFLoaderUpgradeab1e11111111111111111111111","BPFLoader2111111111111111111111111111111111",
 "AddressLookupTab1e1111111111111111111111111","Vote111111111111111111111111111111111111111",
 "Stake11111111111111111111111111111111111111"}
chosen=None
for s in sigs:
    tx=rpc("getTransaction",[s["signature"],{"maxSupportedTransactionVersion":0,"encoding":"json"}])
    if tx and tx["transaction"]["message"].get("addressTableLookups"):
        chosen=(s["signature"],tx); break
sig,tx=chosen
msg=tx["transaction"]["message"]
b64=rpc("getTransaction",[sig,{"maxSupportedTransactionVersion":0,"encoding":"base64"}])
open("gd_tx.b64","w").write(b64["transaction"][0])
static=msg["accountKeys"]
loaded=tx["meta"]["loadedAddresses"]
altaccts=[l["accountKey"] for l in msg["addressTableLookups"]]
allkeys=list(dict.fromkeys(static+loaded["writable"]+loaded["readonly"]+altaccts))
print(f"sig={sig[:12]} static={len(static)} alt_resolved={len(loaded['writable'])+len(loaded['readonly'])} alt_tables={len(altaccts)} total_to_clone={len(allkeys)}")
os.makedirs("gd_accts",exist_ok=True); os.makedirs("gd_progs",exist_ok=True)
manifest={"sig":sig,"blockhash":msg["recentBlockhash"],"cu":tx["meta"]["computeUnitsConsumed"],
  "alt_tables":altaccts,"accounts":[],"programs":[],"builtins":[],
  "pre_token":tx["meta"].get("preTokenBalances"),"post_token":tx["meta"].get("postTokenBalances")}
# batch getMultipleAccounts in chunks of 100
infos={}
for i in range(0,len(allkeys),100):
    chunk=allkeys[i:i+100]
    for k,info in zip(chunk,rpc("getMultipleAccounts",[chunk,{"encoding":"base64"}])["value"]):
        infos[k]=info
for k in allkeys:
    info=infos[k]
    if k in BUILTIN: manifest["builtins"].append(k); continue
    if info is None: manifest["accounts"].append({"pk":k,"missing":True}); continue
    if info["executable"]:
        out=f"gd_progs/{k}.so"
        subprocess.run(["solana","program","dump",k,out,"-u","m"],capture_output=True,text=True)
        ok=os.path.exists(out) and os.path.getsize(out)>0
        manifest["programs"].append({"pk":k,"so":out,"dumped":ok})
        print(f"  prog {k[:8]} dumped={ok} size={os.path.getsize(out) if ok else 0}")
    else:
        json.dump({"pk":k,"lamports":info["lamports"],"owner":info["owner"],"data_b64":info["data"][0]},
                  open(f"gd_accts/{k}.json","w"))
        manifest["accounts"].append({"pk":k,"file":f"gd_accts/{k}.json","owner":info["owner"]})
json.dump(manifest,open("gd_manifest.json","w"),indent=1)
print("clonable_state:",len([a for a in manifest['accounts'] if not a.get('missing')]),
      "missing:",len([a for a in manifest['accounts'] if a.get('missing')]),
      "programs:",len(manifest['programs']),"builtins:",len(manifest['builtins']))
