use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::{collections::HashMap, path::Path, sync::Arc, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
const MODERN: &str = "2026-07-28";
const LEGACY: &str = "2025-11-25";
type Output = Arc<tokio::sync::Mutex<tokio::io::Stdout>>;
pub fn validate_url(raw: &str) -> Result<String> {
    let u = url::Url::parse(raw)?;
    let port = u
        .port()
        .ok_or_else(|| anyhow::anyhow!("Explicit local port required"))?;
    let base = format!("http://127.0.0.1:{port}");
    if port == 0 || !(raw == base || raw == format!("{base}/")) {
        bail!("MCP --url must be http://127.0.0.1:PORT without credentials, path, or parameters");
    }
    Ok(base)
}
fn info() -> Value {
    json!({"name":"allfiles","version":crate::store::VERSION})
}
fn capabilities() -> Value {
    json!({"tools":{"listChanged":false}})
}
fn versions() -> Value {
    json!([MODERN, LEGACY, "2025-06-18", "2025-03-26"])
}
async fn write(out: &Output, id: Value, mut result: Value, error: Option<Value>, modern: bool) {
    let response = if let Some(e) = error {
        json!({"jsonrpc":"2.0","id":id,"error":e})
    } else {
        if modern {
            result["resultType"] = "complete".into();
            result["_meta"] = json!({"io.modelcontextprotocol/serverInfo":info()});
        }
        json!({"jsonrpc":"2.0","id":id,"result":result})
    };
    let mut bytes = serde_json::to_vec(&response).unwrap();
    bytes.push(b'\n');
    let mut w = out.lock().await;
    let _ = w.write_all(&bytes).await;
    let _ = w.flush().await;
}
async fn error(out: &Output, id: Value, code: i64, message: &str) {
    write(
        out,
        id,
        json!({}),
        Some(json!({"code":code,"message":message})),
        false,
    )
    .await;
}
async fn frame(
    reader: &mut tokio::io::BufReader<tokio::io::Stdin>,
    buffer: &mut Vec<u8>,
) -> Result<Option<Vec<u8>>> {
    loop {
        let chunk = reader.fill_buf().await?;
        if chunk.is_empty() {
            return if buffer.is_empty() {
                Ok(None)
            } else {
                Ok(Some(std::mem::take(buffer)))
            };
        }
        let n = chunk
            .iter()
            .position(|b| *b == b'\n')
            .map(|n| n + 1)
            .unwrap_or(chunk.len());
        if buffer.len() + n > 1 << 20 {
            bail!("MCP message exceeds 1 MiB");
        }
        let ended = chunk[n - 1] == b'\n';
        buffer.extend_from_slice(&chunk[..n]);
        reader.consume(n);
        if ended {
            return Ok(Some(std::mem::take(buffer)));
        }
    }
}
pub async fn run(raw: &str) -> Result<()> {
    let bridge = Arc::new(Bridge {
        base: validate_url(raw)?,
        client: reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(25))
            .build()?,
    });
    let out = Arc::new(tokio::sync::Mutex::new(tokio::io::stdout()));
    let mut reader = tokio::io::BufReader::new(tokio::io::stdin());
    let mut buffer = Vec::new();
    let tools: Value = serde_json::from_str(include_str!("mcp_tools.json"))?;
    let mut legacy = String::new();
    let mut ready = false;
    let mut active: HashMap<String, tokio::task::JoinHandle<()>> = HashMap::new();
    let result=async {
        loop{
            let line=tokio::select!{_=crate::shutdown_signal()=>break,line=frame(&mut reader,&mut buffer)=>line?};let Some(line)=line else{break;};if line.iter().all(|b|b.is_ascii_whitespace()){continue;}
            let r:Value=match serde_json::from_slice(&line){Ok(r)=>r,Err(_)=>{error(&out,Value::Null,-32700,"Invalid JSON").await;continue;}};
            let method=r["method"].as_str().unwrap_or("");let id=r.get("id").cloned();
            if !r.is_object()||r["jsonrpc"]!="2.0"||method.is_empty(){error(&out,Value::Null,-32600,"Invalid JSON-RPC request").await;continue;}
            if id.is_none(){match method{"notifications/initialized"=>{if !legacy.is_empty(){ready=true;}},"notifications/cancelled"=>{if let Some(task)=active.remove(&r["params"]["requestId"].to_string()){task.abort();}},_=>{}}continue;}
            let id=id.unwrap();if !(id.is_string()||id.is_i64()||id.is_u64())||id.to_string().len()>256{error(&out,Value::Null,-32600,"Request ID must be a string or integer").await;continue;}
            let params=r.get("params").cloned().unwrap_or(json!({}));if !params.is_object(){error(&out,id,-32602,"Parameters must be an object").await;continue;}
            if params.get("_meta").is_some_and(|v|!v.is_object()){error(&out,id,-32602,"Invalid request metadata").await;continue;}
            let version=&params["_meta"]["io.modelcontextprotocol/protocolVersion"];let modern=!version.is_null()||method=="server/discover";
            if modern{
                if version.as_str().unwrap_or("").is_empty(){error(&out,id,-32602,"Missing per-request protocol version").await;continue;}
                if version!=MODERN{write(&out,id,json!({}),Some(json!({"code":-32022,"message":"Unsupported protocol version","data":{"requested":version,"supported":versions()}})),true).await;continue;}
                if !params["_meta"]["io.modelcontextprotocol/clientCapabilities"].is_object(){error(&out,id,-32602,"Missing per-request client capabilities").await;continue;}
            }
            if method=="initialize"&&!modern{
                if !legacy.is_empty(){error(&out,id,-32600,"Already initialized").await;continue;}
                let version=params["protocolVersion"].as_str().unwrap_or("");
                if version.is_empty()||!params["capabilities"].is_object()||params["clientInfo"]["name"].as_str().unwrap_or("").is_empty()||params["clientInfo"]["version"].as_str().unwrap_or("").is_empty(){error(&out,id,-32602,"initialize requires protocolVersion, capabilities and clientInfo").await;continue;}
                legacy=if [LEGACY,"2025-06-18","2025-03-26"].contains(&version){version}else{LEGACY}.into();
                write(&out,id,json!({"protocolVersion":legacy,"capabilities":capabilities(),"serverInfo":info(),"instructions":include_str!("mcp_instructions.txt")}),None,false).await;continue;
            }
            if method=="ping"&&!modern{write(&out,id,json!({}),None,false).await;continue;}
            if !modern&&!ready{error(&out,id,-32602,"Initialize the MCP connection first").await;continue;}
            match method{
                "server/discover"=>write(&out,id,json!({"supportedVersions":versions(),"capabilities":capabilities(),"instructions":include_str!("mcp_instructions.txt")}),None,modern).await,
                "tools/list"=>{if params.get("cursor").is_some_and(|v|v!=""){error(&out,id,-32602,"Unknown tools cursor").await;}else{write(&out,id,json!({"tools":tools}),None,modern).await;}},
                "tools/call"=>{
                    let name=params["name"].as_str().unwrap_or("");let tool=tools.as_array().unwrap().iter().find(|t|t["name"]==name);
                    if tool.is_none()||params.get("task").is_some(){error(&out,id,-32602,"Unknown tool or unsupported task execution").await;continue;}
                    active.retain(|_,h|!h.is_finished());let key=id.to_string();if active.len()>=8||active.contains_key(&key){error(&out,id,-32600,"Duplicate active request ID or too many concurrent requests").await;continue;}
                    let tool=tool.unwrap().clone();let args=params.get("arguments").cloned().unwrap_or(json!({}));let b=bridge.clone();let out=out.clone();let structured=modern||legacy!="2025-03-26";
                    active.insert(key,tokio::spawn(async move{let result=match b.call(&tool,args).await{Ok(data)=>{let mut v=json!({"content":[{"type":"text","text":data.to_string()}],"isError":false});if structured{v["structuredContent"]=data;}v},Err(e)=>json!({"content":[{"type":"text","text":e.to_string()}],"isError":true})};write(&out,id,result,None,modern).await;}));
                },
                _=>error(&out,id,-32601,"Method not found").await,
            }
        }Ok::<(),anyhow::Error>(())
    }.await;
    for task in active.values() {
        task.abort();
    }
    for (_, task) in active {
        let _ = task.await;
    }
    result
}
pub fn validate(tool: &Value, args: &Value) -> Result<()> {
    let object = args
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("Tool arguments must be an object"))?;
    let schema = &tool["inputSchema"];
    if let Some(required) = schema["required"].as_array() {
        for key in required {
            if !object.contains_key(key.as_str().unwrap()) {
                bail!("Required argument: {key}");
            }
        }
    }
    for (key, value) in object {
        let rule = &schema["properties"][key];
        if rule.is_null() {
            bail!("Unknown argument: {key}");
        }
        match rule["type"].as_str() {
            Some("string") => {
                let text = value
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("{key} must be a string"))?;
                if rule["maxLength"]
                    .as_u64()
                    .is_some_and(|n| text.chars().count() > n as usize)
                    || text.len() > 8192
                {
                    bail!("{key} is too long");
                }
                if rule["enum"]
                    .as_array()
                    .is_some_and(|values| !values.contains(value))
                {
                    bail!("Invalid value for {key}");
                }
            }
            Some("integer") => {
                let n = value
                    .as_i64()
                    .ok_or_else(|| anyhow::anyhow!("{key} must be an integer"))?;
                if rule["minimum"].as_i64().is_some_and(|min| n < min)
                    || rule["maximum"].as_i64().is_some_and(|max| n > max)
                {
                    bail!("{key} is outside its allowed range");
                }
            }
            Some("boolean") if value.is_boolean() => {}
            _ => bail!("Invalid type for {key}"),
        }
    }
    Ok(())
}
struct Bridge {
    base: String,
    client: reqwest::Client,
}
impl Bridge {
    async fn api(&self, path: &str, payload: Option<Value>) -> Result<Value> {
        let mut request = if let Some(payload) = payload {
            let session = self
                .receive(self.client.get(format!("{}/api/session", self.base)))
                .await?;
            let token = session["token"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 128)
                .ok_or_else(|| anyhow::anyhow!("Invalid local session"))?;
            self.client
                .post(format!("{}{path}", self.base))
                .header("x-allfiles-token", token)
                .json(&payload)
        } else {
            self.client.get(format!("{}{path}", self.base))
        };
        request = request.header("origin", &self.base);
        self.receive(request).await
    }
    async fn receive(&self, request: reqwest::RequestBuilder) -> Result<Value> {
        let mut response=request.send().await.map_err(|_|anyhow::anyhow!("AllFiles is unavailable; start the app on its local port and keep the container mounted"))?;
        let status = response.status();
        let mut data = Vec::new();
        while let Some(bytes) = response.chunk().await? {
            if data.len() + bytes.len() > 2 << 20 {
                bail!("API response exceeded 2 MiB; narrow the request");
            }
            data.extend(bytes);
        }
        let v: Value = serde_json::from_slice(&data)?;
        if !status.is_success() {
            bail!(
                "{}",
                v["error"]
                    .as_str()
                    .unwrap_or("Local API request failed")
                    .chars()
                    .take(1000)
                    .collect::<String>()
            );
        }
        Ok(v)
    }
    async fn call(&self, tool: &Value, mut a: Value) -> Result<Value> {
        validate(tool, &a)?;
        let number = |a: &Value, k: &str, d: i64| a[k].as_i64().unwrap_or(d);
        let text = |a: &Value, k: &str| a[k].as_str().unwrap_or("").to_owned();
        match tool["name"].as_str().unwrap() {
            "allfiles_sources" => {
                self.api(
                    &format!(
                        "/api/sources?offset={}&limit={}",
                        number(&a, "offset", 0),
                        number(&a, "limit", 100)
                    ),
                    None,
                )
                .await
            }
            "allfiles_status" => self.api("/api/status", None).await,
            "allfiles_cancel" => self.api("/api/cancel", Some(json!({}))).await,
            "allfiles_search" => {
                let q = url::form_urlencoded::Serializer::new(String::new())
                    .extend_pairs([
                        ("q", text(&a, "query")),
                        (
                            "mode",
                            if a.get("mode").is_some() {
                                text(&a, "mode")
                            } else {
                                "files".into()
                            },
                        ),
                        ("source", text(&a, "source_id")),
                        ("ext", text(&a, "extension")),
                        ("offset", number(&a, "offset", 0).to_string()),
                        ("limit", number(&a, "limit", 20).to_string()),
                    ])
                    .finish();
                self.api(&format!("/api/search?{q}"), None).await
            }
            "allfiles_file" => {
                let offset = number(&a, "offset", 0);
                let limit = number(&a, "max_chars", 4000);
                let v = self
                    .api(
                        &format!(
                            "/api/file?id={}&text_offset={offset}&max_chars={limit}",
                            number(&a, "id", 0)
                        ),
                        None,
                    )
                    .await?;
                let count = v["text"].as_str().unwrap_or("").chars().count() as i64;
                if v["offset"] != offset || count > limit || v["returned_chars"] != count {
                    bail!("Invalid text page; restart AllFiles with the current version");
                }
                Ok(v)
            }
            "allfiles_scan" => {
                if a.get("source_id").is_some() {
                    if ["root", "label", "kind", "location"]
                        .iter()
                        .any(|k| a.get(k).is_some())
                    {
                        bail!("Use source_id alone to preserve source settings");
                    }
                    let id = text(&a, "source_id");
                    if id.is_empty() {
                        bail!("Source ID is empty");
                    }
                    let q = url::form_urlencoded::Serializer::new(String::new())
                        .append_pair("id", &id)
                        .finish();
                    let sources = self.api(&format!("/api/sources?{q}"), None).await?;
                    let src = sources["sources"]
                        .as_array()
                        .and_then(|s| s.iter().find(|s| s["id"] == id))
                        .ok_or_else(|| anyhow::anyhow!("Source ID was not found"))?;
                    for key in ["root", "label", "kind", "location"] {
                        a[key] = src[key].clone();
                    }
                }
                if text(&a, "label").trim().is_empty()
                    || !Path::new(&text(&a, "root")).is_absolute()
                {
                    bail!(
                        "Provide an absolute root and media label; imported offline media must be reimported"
                    );
                }
                if a.get("index_content").is_none() {
                    a["index_content"] = true.into();
                }
                if a.get("kind").is_none() {
                    a["kind"] = "folder".into();
                }
                self.api("/api/scan", Some(a)).await
            }
            _ => bail!("Unknown tool"),
        }
    }
}
