"""Exercise real console interrupts and immediate pairing retries on Windows."""
import json
import os
import pathlib
import shutil
import subprocess
import sys
import threading
import time
import uuid

root = pathlib.Path(__file__).resolve().parents[3]
assert os.name == "nt"
cli = root / "target/release/rustboard.exe"
agent = root / "target/release/rustboardd.exe"
data_root = pathlib.Path(os.environ["LOCALAPPDATA"]) / "Rustboard"

agents, clients, profiles = [], [], []

def invoke(env, *args):
    return subprocess.run([str(cli), *args], env=env, capture_output=True, text=True, timeout=12)

def start_pair(env, target=None):
    startup = subprocess.STARTUPINFO()
    startup.dwFlags = subprocess.STARTF_USESHOWWINDOW
    startup.wShowWindow = 0
    options = dict(creationflags=subprocess.CREATE_NEW_CONSOLE, startupinfo=startup)
    process = subprocess.Popen([str(cli), "pair", *([target] if target else [])], env=env,
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, **options)
    process.output = ""
    def read():
        while value := process.stdout.read(1):
            process.output += value
    threading.Thread(target=read, daemon=True).start()
    clients.append(process)
    return process

def wait_text(process, text):
    deadline = time.monotonic() + 8
    while text not in process.output:
        if process.poll() is not None or time.monotonic() >= deadline:
            raise AssertionError(process.output)
        time.sleep(.02)

def interrupt(process):
    started = time.monotonic()
    # Attach a separate helper to this hidden console; send an actual CTRL_C_EVENT.
    helper = """import ctypes,sys,time
k=ctypes.WinDLL('kernel32',use_last_error=True)
k.FreeConsole()
assert k.AttachConsole(int(sys.argv[1])),ctypes.get_last_error()
assert k.SetConsoleCtrlHandler(None,True),ctypes.get_last_error()
assert k.GenerateConsoleCtrlEvent(0,0),ctypes.get_last_error()
time.sleep(.1)
k.FreeConsole()
"""
    generated = subprocess.run([sys.executable, "-c", helper, str(process.pid)],
        creationflags=subprocess.CREATE_NO_WINDOW, capture_output=True, text=True, timeout=5)
    assert generated.returncode == 0, generated.stderr
    assert process.wait(timeout=3) != 0
    wait_text(process, "pairing cancelled")
    return round((time.monotonic() - started) * 1000, 2)

try:
    for port in (48930, 48934):
        profile = "interrupt-" + uuid.uuid4().hex[:12]
        directory = data_root / profile
        directory.mkdir(mode=0o700)
        env = dict(os.environ, RUSTBOARD_PROFILE=profile)
        profiles.append((env, directory))
        (directory / "config.json").write_text(json.dumps(dict(listen=f"127.0.0.1:{port}", mode="text", identity=None, peers=[])))
        agents.append(subprocess.Popen([str(agent)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            creationflags=subprocess.CREATE_NO_WINDOW))
        for _ in range(60):
            if invoke(env, "status", "--json").returncode == 0:
                break
            time.sleep(.1)
        else:
            raise AssertionError("Agent did not start")
    waiting_times = []
    for _ in range(3):
        first = start_pair(profiles[0][0])
        wait_text(first, "Waiting...")
        waiting_times.append(interrupt(first))
    first = start_pair(profiles[0][0])
    wait_text(first, "Waiting...")
    second = start_pair(profiles[1][0], "127.0.0.1:48930")
    wait_text(first, "Accept? [y/N]")
    wait_text(second, "Accept? [y/N]")
    confirm_time = interrupt(first)
    interrupt(second)
    first = start_pair(profiles[0][0])
    wait_text(first, "Waiting...")
    second = start_pair(profiles[1][0], "127.0.0.1:48930")
    wait_text(first, "Accept? [y/N]")
    wait_text(second, "Accept? [y/N]")
    first.stdin.write("y\n")
    first.stdin.flush()
    wait_text(first, "Confirm on the other peer...")
    approval_time = interrupt(first)
    first = start_pair(profiles[0][0])
    wait_text(first, "Waiting...")
    interrupt(first)
    interrupt(second)
    assert all(not json.loads((directory / "config.json").read_text())["peers"] for _, directory in profiles)
    result = dict(waiting_interrupt_ms=waiting_times, confirmation_interrupt_ms=confirm_time, approval_interrupt_ms=approval_time,
        immediate_retry=True, peers_unchanged=True, native_ctrl_c=True)
    output = root / "test-output/cli-interrupt"
    output.mkdir(parents=True, exist_ok=True)
    (output / "windows.json").write_text(json.dumps(result, indent=2))
    print(json.dumps(result, indent=2))
finally:
    for process in clients + agents:
        if process.poll() is None:
            process.terminate()
            process.wait(timeout=10)
    for _, directory in profiles:
        assert directory.parent == data_root and directory.name.startswith("interrupt-")
        shutil.rmtree(directory)
