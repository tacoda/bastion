"""Spike N14: run one spike binary in each mode, three times, and report cold
start (launch to first paint), its RESULT lines, and resident memory."""
import subprocess, sys, time, threading, os

def run(binary, mode):
    env = dict(os.environ, N14_MODE=mode)
    start = time.time() * 1000
    p = subprocess.Popen([binary], env=env, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
    rss = []
    def sample():
        time.sleep(1.5)
        out = subprocess.run(["ps", "-o", "rss=", "-p", str(p.pid)], capture_output=True, text=True).stdout.strip()
        if out: rss.append(int(out) // 1024)
    threading.Thread(target=sample).start()
    lines = []
    for line in p.stdout:
        line = line.strip()
        if line.startswith("FIRST_PAINT"):
            lines.append("cold_start_ms=%d" % (int(line.split()[1]) - start))
        elif line.startswith("RESULT"):
            lines.append(line[7:])
    p.wait(timeout=60)
    time.sleep(0.1)
    return lines + ["rss_mb=%s" % (rss[0] if rss else "?")]

binary = sys.argv[1]
for mode in ["jobs", "canvas", "diff"]:
    for i in range(3):
        print(mode, i, " | ".join(run(binary, mode)), flush=True)
