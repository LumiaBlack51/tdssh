#!/usr/bin/env python3
"""Package a native executable with runtime scripts, documentation and licenses."""
import argparse
import hashlib
from pathlib import Path
import shutil
import tempfile


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--platform', required=True, choices=['Linux', 'Windows'])
    parser.add_argument('--out', type=Path, default=Path('dist'))
    args = parser.parse_args()
    source = Path(__file__).resolve().parent.parent
    args.out.mkdir(parents=True, exist_ok=True)
    binary_name = 'tdssh.exe' if args.platform == 'Windows' else 'tdssh'
    name = f'tdssh-{args.platform}-x64'
    with tempfile.TemporaryDirectory(prefix='tdssh-package-', dir=args.out) as temp:
        root = Path(temp) / 'tdssh'
        root.mkdir()
        shutil.copyfile(args.binary, root / binary_name)
        (root / binary_name).chmod(0o755)
        for file in ['README.md', 'LICENSE', 'THIRD_PARTY.md']:
            shutil.copyfile(source / file, root / file)
        for folder in ['examples', 'docs', 'licenses']:
            shutil.copytree(source / folder, root / folder)
        licenses = root / 'licenses' / 'netwatch'
        licenses.mkdir()
        for file in (source / 'vendor' / 'netwatch').glob('LICENSE-*'):
            shutil.copyfile(file, licenses / file.name)
        scripts = root / 'scripts'
        scripts.mkdir()
        names = ['install.sh', 'install-service.sh'] if args.platform == 'Linux' else ['install.ps1', 'install-service.ps1']
        for file in names:
            target = scripts / file
            shutil.copyfile(source / 'scripts' / file, target)
            target.chmod(0o755 if file.endswith('.sh') else 0o644)
        digest = hashlib.sha256((root / binary_name).read_bytes()).hexdigest()
        (root / 'SHA256SUMS.txt').write_text(f'{digest}  {binary_name}\n')
        archive = shutil.make_archive(str(args.out.resolve() / name), 'zip' if args.platform == 'Windows' else 'gztar', temp, 'tdssh')
    archive = Path(archive)
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    archive.with_name(archive.name + '.sha256').write_text(f'{digest}  {archive.name}\n')
    print(archive)

if __name__ == '__main__':
    main()
