#!/usr/bin/env python3
"""Synthetic MCP peer: records negotiation; advertises an inert UI resource."""

import json
import os
from pathlib import Path
import sys

for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    if "id" not in request:
        continue
    if method == "initialize":
        Path(sys.argv[1]).write_text(json.dumps(request["params"], indent=2))
        Path(sys.argv[1]).with_suffix(".pid").write_text(str(os.getpid()))
        result = {
            "protocolVersion": "2025-11-25",
            "serverInfo": {"name": "rostfrei-synthetic-mcp-peer", "version": "0.0.0"},
            "capabilities": {"resources": {}, "tools": {}},
        }
    elif method == "resources/list":
        result = {"resources": [{
            "uri": "ui://rostfrei/synthetic",
            "name": "synthetic_ui_fixture",
            "mimeType": "text/html;profile=mcp-app",
            "description": "Synthetic discovery probe, not a rendered MCP App",
        }]}
    elif method == "resources/templates/list":
        result = {"resourceTemplates": []}
    elif method == "tools/list":
        result = {"tools": [{
            "name": "synthetic_view",
            "description": "Synthetic read-only presentation discovery probe",
            "inputSchema": {"type": "object", "properties": {}, "additionalProperties": False},
            "_meta": {"ui": {"resourceUri": "ui://rostfrei/synthetic"}},
        }]}
    elif method == "tools/call":
        result = {"content": [{"type": "text", "text": "Synthetic fixture; no Tracer execution"}]}
    elif method == "resources/read":
        result = {"contents": [{
            "uri": "ui://rostfrei/synthetic", "mimeType": "text/html;profile=mcp-app",
            "text": "<!doctype html><p>Synthetic adapter fixture; no Tracer execution</p>",
        }]}
    elif method == "ping":
        result = {}
    else:
        print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "error": {"code": -32601, "message": "Unsupported fixture method"}}), flush=True)
        continue
    print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}), flush=True)
