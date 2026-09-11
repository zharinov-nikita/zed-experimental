p = 'LOCAL_DEV.md'
data = open(p, 'rb').read()
broken = b'com.pais.handy' + b'\x0d' + b'ecordings'
fixed = b'com.pais.handy' + b'\x5c' + b'recordings'
assert broken in data, 'pattern not found'
data = data.replace(broken, fixed)
open(p, 'wb').write(data)
print('fixed')
