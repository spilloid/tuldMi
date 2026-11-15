"""Check native backend feature routing without compiling either target's SDK code."""
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent

for target, backend in [('x86_64-pc-windows-msvc', 'vulkan'), ('aarch64-apple-darwin', 'metal')]:
    for gpu in (True, False):
        command = ['cargo', 'tree', '--locked', '--target', target,
                   '-p', 'tuldmi', '-e', 'features', '--prefix', 'none', '-i', 'whisper-rs-sys']
        if not gpu:
            command.append('--no-default-features')
        result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True)
        if result.returncode:
            raise SystemExit(result.stderr)
        active = {name for name in ('metal', 'vulkan')
                  if f'whisper-rs-sys feature "{name}"' in result.stdout}
        expected = {backend} if gpu else set()
        if active != expected:
            raise SystemExit(f'{target} gpu={gpu}: expected {expected}, got {active}\n{result.stdout}')
        print(f'{target}: {backend if gpu else "CPU only"} feature routing OK')
