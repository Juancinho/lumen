"""Paginate a list endpoint by cursor."""

def fetch_all(client, path):
    items, cursor = [], None
    while True:
        page = client.get(path, params={"cursor": cursor})
        items += page["items"]
        cursor = page.get("next")
        if not cursor:
            return items
