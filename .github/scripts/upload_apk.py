import os
import sys
import json
import urllib.request
import urllib.error

def main():
    token = os.environ.get("GITHUB_TOKEN")
    repo = os.environ.get("GITHUB_REPOSITORY")
    tag = os.environ.get("GITHUB_REF_NAME")

    if not token:
        print("Error: GITHUB_TOKEN environment variable is missing.")
        sys.exit(1)
    if not repo:
        print("Error: GITHUB_REPOSITORY environment variable is missing.")
        sys.exit(1)
    if not tag:
        print("Error: GITHUB_REF_NAME environment variable is missing.")
        sys.exit(1)

    apk_path = "NexusCompanion.apk"
    if not os.path.exists(apk_path):
        print(f"Error: {apk_path} not found in workspace.")
        sys.exit(1)

    apk_size = os.path.getsize(apk_path)
    print(f"Found {apk_path} ({apk_size} bytes). Preparing release asset upload for tag '{tag}'...")

    headers = {
        "Accept": "application/vnd.github+json",
        "Authorization": f"Bearer {token}",
        "User-Agent": "Nexus-Release-Uploader",
    }

    # Fetch release by tag
    rel_url = f"https://api.github.com/repos/{repo}/releases/tags/{tag}"
    req = urllib.request.Request(rel_url, headers=headers)
    try:
        with urllib.request.urlopen(req) as resp:
            release = json.loads(resp.read().decode())
    except urllib.error.HTTPError as e:
        print(f"Failed to fetch release for tag {tag}: {e.code} {e.reason}")
        print(e.read().decode())
        sys.exit(1)

    release_id = release.get("id")
    print(f"Target release ID: {release_id} ('{release.get('name')}')")

    # If NexusCompanion.apk already exists as an asset, delete it first to overwrite
    for asset in release.get("assets", []):
        if asset["name"] == "NexusCompanion.apk":
            print(f"Found existing asset ID {asset['id']}. Deleting before replacing...")
            del_req = urllib.request.Request(asset["url"], headers=headers, method="DELETE")
            try:
                urllib.request.urlopen(del_req)
                print("Deleted existing asset.")
            except Exception as e:
                print(f"Warning: could not delete existing asset: {e}")
            break

    # Upload new asset
    upload_url_raw = release.get("upload_url", "")
    upload_url = upload_url_raw.split("{")[0] + "?name=NexusCompanion.apk"

    print(f"Uploading {apk_path} to {upload_url}...")
    with open(apk_path, "rb") as f:
        apk_data = f.read()

    upload_headers = {
        "Accept": "application/vnd.github+json",
        "Authorization": f"Bearer {token}",
        "Content-Type": "application/vnd.android.package-archive",
        "User-Agent": "Nexus-Release-Uploader",
    }

    up_req = urllib.request.Request(upload_url, data=apk_data, headers=upload_headers)
    try:
        with urllib.request.urlopen(up_req) as resp:
            result = json.loads(resp.read().decode())
            print(f"Successfully uploaded {apk_path}!")
            print("Download URL:", result.get("browser_download_url"))
    except urllib.error.HTTPError as e:
        print(f"Failed to upload asset: {e.code} {e.reason}")
        print(e.read().decode())
        sys.exit(1)

if __name__ == "__main__":
    main()
