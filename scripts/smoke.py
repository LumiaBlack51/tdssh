#!/usr/bin/env python3
"""Real two-process QUIC transfer through the CLI; no real SSH credentials required."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import threading
import time


def wait_file(path, process, timeout=55):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if path.exists():
            return path.read_text().strip()
        if process.poll() is not None:
            raise RuntimeError(f'process exited {process.returncode} before readiness')
        time.sleep(0.05)
    raise TimeoutError(f'waiting for {path.name}')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', default='target/debug/tdssh' + ('.exe' if os.name == 'nt' else ''))
    parser.add_argument('--live-relay', action='store_true')
    parser.add_argument('--force-direct', action='store_true')
    args = parser.parse_args()
    binary = str(Path(args.binary).resolve())
    processes = []
    logs = []
    with tempfile.TemporaryDirectory(prefix='tdssh-smoke-') as root:
        root = Path(root)
        def run(state, *extra):
            return subprocess.check_output([binary, '--state-dir', str(root / state), *extra], text=True, timeout=15).strip()
        client_id = run('client', 'init')
        server_id = run('server', 'init')
        run('server', 'allow', client_id)
        network = ['--relay-only'] if args.live_relay else ['--no-relay']
        if args.force_direct:
            network += ['--force-direct']
        def start(state, *extra):
            log = open(root / f'{state}-{len(logs)}.log', 'w+', encoding='utf-8')
            logs.append(log)
            proc = subprocess.Popen([binary, '--state-dir', str(root / state), *network, *extra], stdout=subprocess.DEVNULL, stderr=log)
            processes.append(proc)
            return proc
        listener = socket.socket()
        listener.bind(('127.0.0.1', 0))
        listener.listen()
        listener.settimeout(90)
        payload = bytes(range(251)) * 16711
        failures = []
        def backend():
            try:
                connection, _ = listener.accept()
                with connection:
                    connection.settimeout(30)
                    data = bytearray()
                    while chunk := connection.recv(65536):
                        data.extend(chunk)
                    assert data == payload, 'upload differs'
                    connection.sendall(data)
                    connection.shutdown(socket.SHUT_WR)
            except BaseException as error:
                failures.append(error)
        worker = threading.Thread(target=backend, daemon=True)
        worker.start()
        try:
            ticket = root / 'server.ticket'
            server = start('server', 'serve', '--target', f'127.0.0.1:{listener.getsockname()[1]}', '--ticket-out', str(ticket))
            wait_file(ticket, server)
            ready = root / 'forward.ready'
            client = start('client', 'forward', '--peer', server_id, '--ticket-file', str(ticket), '--listen', '127.0.0.1:0', '--ready-file', str(ready))
            address = wait_file(ready, client)
            host, port = address.rsplit(':', 1)
            with socket.create_connection((host, int(port)), timeout=30) as connection:
                connection.settimeout(40)
                connection.sendall(payload)
                connection.shutdown(socket.SHUT_WR)
                result = bytearray()
                while chunk := connection.recv(65536):
                    result.extend(chunk)
            worker.join(timeout=5)
            assert not worker.is_alive(), 'backend did not finish'
            if failures:
                raise failures[0]
            assert result == payload, 'download differs'
            assert server.poll() is None and client.poll() is None
            print(json.dumps({'passed': True, 'mode': 'public-relay' if args.live_relay else 'local-udp', 'force_direct': args.force_direct, 'bytes_each_direction': len(payload), 'sha256': hashlib.sha256(payload).hexdigest(), 'independent_nat_tested': False}))
        except BaseException:
            for log in logs:
                log.flush(); log.seek(0)
                print(log.read())
            raise
        finally:
            listener.close()
            for process in processes:
                if process.poll() is None:
                    process.terminate()
            for process in processes:
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill(); process.wait(timeout=5)
            for log in logs:
                log.close()

if __name__ == '__main__':
    main()
