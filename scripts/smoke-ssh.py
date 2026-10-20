#!/usr/bin/env python3
"""Linux: isolated unprivileged OpenSSH server, shell/scp/ProxyCommand/exit-code checks."""
import argparse
import getpass
import hashlib
import json
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import time
from smoke import wait_file


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', default='target/debug/tdssh')
    parser.add_argument('--force-direct', action='store_true')
    parser.add_argument('--live-relay', action='store_true')
    args = parser.parse_args()
    binary = str(Path(args.binary).resolve())
    sshd = shutil.which('sshd') or '/usr/sbin/sshd'
    processes, logs = [], []
    with tempfile.TemporaryDirectory(prefix='tdssh-openssh-') as name:
        root = Path(name)
        def command(*argv, check=True, **options):
            return subprocess.run([str(x) for x in argv], check=check, capture_output=True, timeout=60, **options)
        for key in ['host', 'client-key']:
            command('ssh-keygen', '-q', '-t', 'ed25519', '-N', '', '-f', root / key)
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0)); port = sock.getsockname()[1]
        config = root / 'sshd_config'
        config.write_text(f'''Port {port}
ListenAddress 127.0.0.1
HostKey {root / 'host'}
PidFile {root / 'sshd.pid'}
AuthorizedKeysFile {root / 'client-key.pub'}
StrictModes no
PasswordAuthentication no
KbdInteractiveAuthentication no
UsePAM no
PermitRootLogin prohibit-password
AllowUsers {getpass.getuser()}
Subsystem sftp internal-sftp
LogLevel ERROR
''')
        network = ['--relay-only'] if args.live_relay else ['--no-relay']
        if args.force_direct: network += ['--force-direct']
        def td(state, *argv): return [binary, '--state-dir', str(root / state), *argv]
        def start(argv):
            log = open(root / f'process-{len(logs)}.log', 'w+')
            logs.append(log)
            process = subprocess.Popen(argv, stdout=subprocess.DEVNULL, stderr=log)
            processes.append(process)
            return process
        try:
            sshd_process = start([sshd, '-D', '-e', '-f', str(config)])
            for _ in range(50):
                if sshd_process.poll() is not None: raise RuntimeError('isolated sshd exited')
                try:
                    with socket.create_connection(('127.0.0.1', port), timeout=0.2): pass
                    break
                except OSError: time.sleep(0.05)
            client_id = command(*td('client', 'init'), text=True).stdout.strip()
            server_id = command(*td('server', 'init'), text=True).stdout.strip()
            command(*td('server', 'allow', client_id))
            ticket = root / 'ticket'
            server = start(td('server', *network, 'serve', '--target', f'127.0.0.1:{port}', '--ticket-out', str(ticket)))
            wait_file(ticket, server)
            ready = root / 'ready'
            forward = start(td('client', *network, 'forward', '--peer', server_id, '--ticket-file', str(ticket), '--listen', '127.0.0.1:0', '--ready-file', str(ready)))
            address = wait_file(ready, forward)
            local_port = address.rsplit(':', 1)[1]
            options = ['-i', str(root / 'client-key'), '-o', 'BatchMode=yes', '-o', 'StrictHostKeyChecking=accept-new', '-o', f'UserKnownHostsFile={root / "known_hosts"}', '-o', f'HostKeyAlias=tdssh-{server_id}']
            destination = f'{getpass.getuser()}@127.0.0.1'
            shell = command('ssh', *options, '-p', local_port, destination, 'printf tdssh-real-ssh', text=True)
            assert shell.stdout == 'tdssh-real-ssh'
            payload = bytes(range(251)) * 8191
            (root / 'upload.bin').write_bytes(payload)
            command('scp', *options, '-P', local_port, root / 'upload.bin', f'{destination}:{root / "remote.bin"}')
            command('scp', *options, '-P', local_port, f'{destination}:{root / "remote.bin"}', root / 'download.bin')
            assert (root / 'download.bin').read_bytes() == payload
            # A native ssh child retains argv, remote command and exit status.
            second_id = command(*td('client-stdio', 'init'), text=True).stdout.strip()
            command(*td('server', 'allow', second_id))
            native_options = []
            for option in options: native_options += [f'--ssh-option={option}']
            result = command(*td('client-stdio', *network, 'ssh', '--peer', server_id, '--ticket-file', str(ticket), '--user', getpass.getuser(), *native_options, '--', 'exit 37'), check=False)
            assert result.returncode == 37, (result.returncode, result.stderr.decode())
            # Exercise actual stdin/stdout ProxyCommand bytes, not just TCP forwards.
            import shlex
            proxy = shlex.join(td('client-stdio', *network, 'proxy', '--peer', server_id, '--ticket-file', str(ticket)))
            result = command('ssh', *options, '-o', f'ProxyCommand={proxy}', destination, 'printf tdssh-proxy', text=True)
            assert result.stdout == 'tdssh-proxy'
            print(json.dumps({'passed': True, 'openssh_shell': True, 'scp_sftp_bytes': len(payload), 'sha256': hashlib.sha256(payload).hexdigest(), 'native_ssh_exit_code': 37, 'proxy_command': True, 'force_direct': args.force_direct, 'public_relay': args.live_relay, 'independent_nat_tested': False}))
        except BaseException:
            for log in logs:
                log.flush(); log.seek(0); print(log.read())
            raise
        finally:
            for process in processes:
                if process.poll() is None: process.terminate()
            for process in processes:
                try: process.wait(timeout=10)
                except subprocess.TimeoutExpired: process.kill(); process.wait(timeout=5)
            for log in logs: log.close()

if __name__ == '__main__': main()
