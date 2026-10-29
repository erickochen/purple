#!/usr/bin/env python3
"""Offline PTY end-to-end test: real Purple, fake bw/SSH, synthetic credentials."""
import json, os, pathlib, pty, select, subprocess, sys, tempfile, termios, time
binary = str(pathlib.Path(sys.argv[1]).resolve())
with tempfile.TemporaryDirectory(prefix='purple-bw-smoke-') as temporary:
    root = pathlib.Path(temporary)
    mock = root/'bin'; mock.mkdir()
    state = root/'vault.json'
    state.write_text(json.dumps({'items': [], 'creates': 0, 'calls': []}))
    bw = mock/'bw'
    bw.write_text('''#!/usr/bin/env python3
import os,json,sys,base64
from pathlib import Path
p=Path(os.environ['SMOKE_VAULT']);s=json.loads(p.read_text());a=sys.argv[1:]
s['calls'].append(a[:2]);p.write_text(json.dumps(s))
if a==['status']: print(json.dumps({'status':'unlocked'}))
elif a==['sync']: print('Sync complete')
elif a[:2]==['list','items']: print(json.dumps(s['items']))
elif a==['list','folders']: print(json.dumps([{'id':'22222222-2222-4222-8222-222222222222','name':'ssh/Purple'}]))
elif a==['create','item']:
 item=json.loads(base64.b64decode(sys.stdin.read()));item['id']='11111111-1111-4111-8111-111111111111';s['items'].append(item);s['creates']+=1;p.write_text(json.dumps(s));print(json.dumps(item))
elif a[:2]==['get','item']: print(json.dumps(s['items'][0]))
elif a[:2]==['get','password']: print(s['items'][0]['login']['password'])
else: sys.exit(9)
''');bw.chmod(0o755)
    ssh = mock/'fake-ssh'
    ssh.write_text('''#!/usr/bin/env python3
import os,subprocess
secret=subprocess.check_output([os.environ['SSH_ASKPASS'],"root@example.test's password: "],env=os.environ).decode().strip()
assert secret=='test-only-password'
print('SMOKE_AUTH_OK')
''');ssh.chmod(0o755)
    config=root/'config';config.write_text('Host enroll-test\n HostName example.test\n User root\n # purple:askpass bw:\n')
    env=dict(os.environ,PATH=str(mock)+':'+os.environ['PATH'],SMOKE_VAULT=str(state),PURPLE_SSH_COMMAND=str(ssh),TERM='xterm-256color')
    for category in ['CONFIG','DATA','STATE','CACHE']:env['PURPLE_'+category+'_DIR']=str(root/'purple')
    for attempt in range(2):
        master,slave=pty.openpty();p=subprocess.Popen([binary,'--config',str(config),'--connect','enroll-test'],stdin=slave,stdout=slave,stderr=slave,env=env,close_fds=True);os.close(slave)
        data=b'';sent=False;deadline=time.monotonic()+40
        while time.monotonic()<deadline:
            if select.select([master],[],[],0.1)[0]:
                try:chunk=os.read(master,65536)
                except OSError:break
                if not chunk:break
                data+=chunk
            if 'Server login password'.encode() in data and not sent and not (termios.tcgetattr(master)[3] & termios.ECHO):
                assert attempt==0,'second connection must reuse UUID'
                os.write(master,b'test-only-password\r');sent=True
            if p.poll() is not None:break
        try:p.wait(timeout=1)
        except subprocess.TimeoutExpired:p.kill();p.wait()
        os.close(master)
        assert p.returncode==0, data.decode(errors='replace')
        assert b'SMOKE_AUTH_OK' in data, data.decode(errors='replace')
        assert b'test-only-password' not in data,'password leaked to terminal'
    result=json.loads(state.read_text());assert result['creates']==1
    assert result['calls'].count(['sync'])==1, result['calls']
    assert result['calls'].count(['status'])==2, result['calls']
    assert result['calls'].count(['get','password'])==2, result['calls']
    assert 'bw:11111111-1111-4111-8111-111111111111' in config.read_text()
    for path in (root/'purple').rglob('*'):
        if path.is_file():assert b'test-only-password' not in path.read_bytes(),str(path)
    print('PASS: first login created one Bitwarden item via stdin, saved UUID, authenticated through Purple ASKPASS; second login reused it; no password in terminal/config/logs.')

# Legacy name/search sources must keep their original connection behavior.
with tempfile.TemporaryDirectory(prefix='purple-bw-legacy-') as temporary:
    root = pathlib.Path(temporary); mock = root/'bin'; mock.mkdir()
    bw = mock/'bw'
    bw.write_text("#!/bin/sh\nif [ \"$1\" = status ]; then printf '{\"status\":\"unlocked\"}'; else printf 'legacy-test-password'; fi\n")
    bw.chmod(0o755)
    ssh = mock/'ssh-test'
    ssh.write_text("#!/bin/sh\nvalue=$(\"$SSH_ASKPASS\" \"root@example.test's password: \")\n[ \"$value\" = legacy-test-password ] && printf LEGACY_OK\n")
    ssh.chmod(0o755)
    config=root/'config'; config.write_text('Host legacy\n HostName example.test\n User root\n # purple:askpass bw:my-server-item\n')
    env=dict(os.environ, PATH=str(mock)+':'+os.environ['PATH'], PURPLE_SSH_COMMAND=str(ssh))
    for category in ['CONFIG','DATA','STATE','CACHE']:env['PURPLE_'+category+'_DIR']=str(root/'purple')
    result=subprocess.run([binary,'--config',str(config),'--connect','legacy'],env=env,capture_output=True,timeout=20)
    assert result.returncode==0 and b'LEGACY_OK' in result.stdout, result.stderr
    assert b'Invalid Bitwarden ID' not in result.stderr and b'legacy-test-password' not in result.stdout+result.stderr
    print('PASS: legacy bw:item-name connects without managed-rename warnings')
