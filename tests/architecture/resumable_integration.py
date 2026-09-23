#!/usr/bin/env python3
"""Isolated real AgentWay+tusd integration; never uses live credentials/providers.

Build first: docker compose build app media
Run: python3 tests/architecture/resumable_integration.py
Creates and removes its own network, containers and volumes. Tests process
failure, storage write denial and consistent two-volume backup/restore. This
is not a physical power-loss or ENOSPC test.
"""
import base64
import concurrent.futures
import hashlib
import json
import subprocess
import time
import urllib.error
import urllib.request
import uuid

PREFIX = 'agentway-transfer-test-' + uuid.uuid4().hex[:10]
containers, volumes = [], []


def docker(*args):
    return subprocess.check_output(['docker', *args], text=True, stderr=subprocess.PIPE).strip()


def volume(suffix):
    name = PREFIX + '-' + suffix
    docker('volume', 'create', name)
    volumes.append(name)
    return name


def request(base, path, method='GET', data=None, token=None, headers=None):
    if isinstance(data, dict):
        data = json.dumps(data).encode()
    req = urllib.request.Request(base + path, method=method, data=data, headers={
        'Content-Type': 'application/json', **({'Authorization': 'Bearer ' + token} if token else {}),
        **(headers or {})})
    try:
        response = urllib.request.urlopen(req, timeout=150)
    except urllib.error.HTTPError as e:
        response = e
    with response:
        return response.status, dict(response.headers), response.read()


def wait_ready(base):
    for _ in range(120):
        try:
            if request(base, '/health/media')[0] == 200:
                return
        except (OSError, urllib.error.URLError):
            pass
        time.sleep(0.25)
    raise AssertionError('Test services did not become ready')


def start(data_volume, media_volume):
    media, app = PREFIX + '-media', PREFIX + '-app'
    docker('run', '-d', '--name', media, '--network', PREFIX, '--network-alias', 'media',
           '-v', media_volume + ':/srv/tusd-data', 'agentway-media:latest',
           '-upload-dir=/srv/tusd-data', '-hooks-dir=/srv/tusd-hooks',
           '-hooks-enabled-events=pre-create', '-disable-download', '-disable-cors', '-max-size=2147483648')
    containers.append(media)
    docker('run', '-d', '--name', app, '--network', PREFIX,
           '-p', '127.0.0.1::8787', '-p', '127.0.0.1::8788',
           '-e', 'TUSD_URL=http://media:8080', '-e', 'TUSD_DATA_DIR=/tus-data',
           '-v', data_volume + ':/data', '-v', media_volume + ':/tus-data:ro', 'agentway-app:latest')
    containers.append(app)
    admin = 'http://127.0.0.1:' + docker('port', app, '8787/tcp').rsplit(':', 1)[1]
    bridge = 'http://127.0.0.1:' + docker('port', app, '8788/tcp').rsplit(':', 1)[1]
    wait_ready(admin)
    return admin, bridge, app, media


def main():
    docker('network', 'create', PREFIX)
    try:
        data, storage, backup = volume('data'), volume('media'), volume('backup')
        admin, bridge, app, media = start(data, storage)
        token = json.loads(request(admin, '/api/publishing/token', 'POST', {})[2])['token']

        def api(path, method='GET', data=None):
            status, _, body = request(bridge, path, method, data, token)
            return status, json.loads(body) if body else None

        def create(payload):
            args = dict(request_id=str(uuid.uuid4()), size=len(payload), mime='video/mp4',
                        sha256=hashlib.sha256(payload).hexdigest())
            status, upload = api('/v1/media/uploads', 'POST', args)
            assert status == 200, (status, upload)
            assert api('/v1/media/uploads', 'POST', args)[1]['media_id'] == upload['media_id']
            return upload

        def patch(upload, offset, payload):
            return request(bridge, upload['upload_path'], 'PATCH', payload, token, {
                'Tus-Resumable': '1.0.0', 'Upload-Offset': str(offset),
                'Content-Type': 'application/offset+octet-stream',
                'Upload-Checksum': 'sha256 ' + base64.b64encode(hashlib.sha256(payload).digest()).decode()})[0]

        upload = create(b'abcd')
        with concurrent.futures.ThreadPoolExecutor(2) as pool:
            outcomes = list(pool.map(lambda _: patch(upload, 0, b'ab'), range(2)))
        assert sorted(outcomes) == [204, 409], outcomes
        assert api(upload['status_path'])[1]['offset'] == 2
        print('Real tusd: duplicate creation and concurrent offset retry passed.', flush=True)

        docker('kill', '--signal=KILL', media)
        assert api(upload['status_path'])[0] == 503
        assert request(admin, '/health/media')[0] == 503
        docker('start', media)
        wait_ready(admin)
        assert api(upload['status_path'])[1]['offset'] == 2

        # Fault-inject a denied write to this isolated transfer file, not live storage.
        files = docker('exec', media, 'find', '/srv/tusd-data', '-maxdepth', '1', '-type', 'f').splitlines()
        binary = next(f for f in files if not f.endswith('.info') and not f.endswith('.lock'))
        docker('exec', media, 'chmod', '444', binary)
        assert patch(upload, 2, b'cd') == 503
        assert api(upload['status_path'])[1]['ready'] is False
        assert api(upload['status_path'])[1]['offset'] == 2
        docker('exec', media, 'chmod', '664', binary)
        print('Unavailable tusd and denied storage write preserve recoverable state.', flush=True)

        # Quiesce both writers; backup database/key/legacy files and transport volume.
        docker('stop', app, media)
        docker('run', '--rm', '-v', data + ':/source-data:ro', '-v', storage + ':/source-media:ro',
               '-v', backup + ':/backup', 'alpine:3', 'sh', '-c',
               'tar -czf /backup/data.tgz -C /source-data . && tar -czf /backup/media.tgz -C /source-media .')
        docker('rm', app, media)
        containers.clear()
        restored_data, restored_media = volume('restored-data'), volume('restored-media')
        docker('run', '--rm', '-v', restored_data + ':/data', '-v', restored_media + ':/media',
               '-v', backup + ':/backup:ro', 'alpine:3', 'sh', '-c',
               'tar -xzf /backup/data.tgz -C /data && tar -xzf /backup/media.tgz -C /media')
        admin, bridge, app, media = start(restored_data, restored_media)
        assert json.loads(request(admin, '/api/publishing/token', 'POST', {})[2])['token'] == token
        assert api(upload['status_path'])[1]['offset'] == 2
        assert patch(upload, 2, b'cd') == 204
        status, result = api(upload['complete_path'], 'POST', {})
        assert status == 200 and result['ready'], (status, result)
        assert api(upload['complete_path'], 'POST', {})[1]['ready']
        assert api(upload['status_path'], 'DELETE')[1]['status'] == 'cancelled'
        print('Two-volume restore preserved credential, media ID and partial bytes; completion and deletion passed.', flush=True)

        corrupt = create(b'abcd')
        assert patch(corrupt, 0, b'wrong'[:4]) == 204
        assert api(corrupt['complete_path'], 'POST', {})[0] == 422
        assert api(corrupt['status_path'], 'DELETE')[0] == 200
        print('Final integrity mismatch blocks readiness. No provider calls made.', flush=True)
    finally:
        for name in containers:
            subprocess.run(['docker', 'rm', '-f', name], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for name in volumes:
            subprocess.run(['docker', 'volume', 'rm', name], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        subprocess.run(['docker', 'network', 'rm', PREFIX], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


if __name__ == '__main__':
    main()
