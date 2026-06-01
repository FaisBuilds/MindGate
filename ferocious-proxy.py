#!/usr/bin/env python3
"""
Ferocious Proxy — mitmproxy addon
Blocks URLs based on config.json
"""

from mitmproxy import http
import json
import os

CONFIG_FILE = "/etc/ferocious/config.json"

def load_config():
    try:
        with open(CONFIG_FILE, 'r') as f:
            return json.load(f)
    except:
        return {"blocked_domains": [], "blocked_keywords": [], "blocked_subreddits": []}

def request(flow: http.HTTPFlow) -> None:
    config = load_config()
    url = flow.request.pretty_url.lower()
    
    # Block by domain
    for domain in config.get("blocked_domains", []):
        if domain in url:
            flow.response = http.Response.make(403, b"Blocked by Ferocious")
            return
    
    # Block by keyword
    for keyword in config.get("blocked_keywords", []):
        if keyword in url:
            flow.response = http.Response.make(403, b"Blocked by Ferocious")
            return
    
    # Block specific subreddits
    for subreddit in config.get("blocked_subreddits", []):
        if f"reddit.com/r/{subreddit}" in url:
            flow.response = http.Response.make(403, b"Subreddit blocked by Ferocious")
            return
