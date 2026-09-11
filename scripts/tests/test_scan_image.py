import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class ImageScanTests(unittest.TestCase):
    def test_scanner_failure_is_preserved_and_reports_are_retained(self):
        for status in (0, 7):
            with self.subTest(status=status), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                (root / 'scripts').mkdir()
                for name in ('scan_image.sh', 'record_build.py'):
                    shutil.copyfile(ROOT / 'scripts' / name, root / 'scripts' / name)
                (root / 'Cargo.lock').write_text('version = 4\n')
                binary = root / 'bin'
                binary.mkdir()
                docker = binary / 'docker'
                docker.write_text('''#!/usr/bin/env python3
import json, os, pathlib, sys
args = sys.argv[1:]
with open(os.environ['CALL_LOG'], 'a') as log: log.write(json.dumps(args) + '\\n')
if args[0] == 'save': pathlib.Path(args[args.index('-o')+1]).write_bytes(b'image')
elif args[0] == 'image': print('sha256:' + '1' * 64)
elif args[0] == 'run':
    mount = pathlib.Path(args[args.index('-v')+1].removesuffix(':/scan'))
    output = args[args.index('--output')+1].removeprefix('/scan/')
    (mount / output).write_text('{}')
    if output == 'vulnerabilities.json': sys.exit(int(os.environ['SCAN_STATUS']))
else: sys.exit(99)
''')
                docker.chmod(0o755)
                git = binary / 'git'
                git.write_text('#!/usr/bin/env python3\nimport sys\nif "status" not in sys.argv: print("2" * 40)\n')
                git.chmod(0o755)
                environment = dict(os.environ, PATH=str(binary) + os.pathsep + os.environ['PATH'],
                                   CALL_LOG=str(root / 'calls.jsonl'), SCAN_STATUS=str(status), TMPDIR=str(root))
                result = subprocess.run(['bash', str(root / 'scripts/scan_image.sh')],
                                        cwd=root, env=environment, capture_output=True, text=True)
                self.assertEqual(result.returncode, status, result.stderr)
                record = json.loads((root / 'artifacts/build-record.json').read_text())
                self.assertFalse(record['signed'])
                self.assertEqual(set(record['files']), {'sbom.cdx.json', 'rust-sbom.cdx.json', 'vulnerabilities.json'})
                calls = [json.loads(line) for line in (root / 'calls.jsonl').read_text().splitlines()]
                self.assertFalse(any('docker.sock' in argument for call in calls for argument in call))
                mounts = [Path(call[call.index('-v') + 1].removesuffix(':/scan'))
                          for call in calls if call[0] == 'run']
                self.assertTrue(mounts)
                self.assertTrue(all(not mount.exists() for mount in mounts))
                self.assertFalse(any(call[0] in ('push', 'login') for call in calls))


if __name__ == '__main__':
    unittest.main()
