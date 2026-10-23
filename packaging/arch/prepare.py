#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Create a pinned, checksummed local Arch source kit from a clean Git commit."""
import argparse
import gzip
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[2]


def git(*args):
    return subprocess.check_output(['git', '-C', str(ROOT), *args])


# Agent tooling versioned for development only: excluded from the source kit,
# never shipped. Any other private or proprietary path still aborts the kit.
DEVELOPMENT_ONLY = ('.agents', '.claude', 'skills-lock.json')
RECIPES = {'legacy': 'packaging/arch/PKGBUILD', 'native': 'packaging/arch-native/PKGBUILD'}


def development_only(name):
    return PurePosixPath(name).parts[:1] in [(p,) for p in DEVELOPMENT_ONLY]


def check_distributable(names):
    """Reject proprietary or private files among those the kit would ship."""
    forbidden = {'.exe','.dll','.msi','.pdb','.so','.pyc'}
    private = {'.git','.agents','.codex','.claude','.aws','node_modules','target','research-artifacts'}
    for name in filter(None,names):
        if development_only(name):
            continue
        path=PurePosixPath(name)
        if path.suffix.lower() in forbidden or private.intersection(path.parts):
            raise RuntimeError(f'Archivo no distribuible en el commit: {name}')


def prepare(output, variant='legacy'):
    if subprocess.run(['git','-C',str(ROOT),'diff-index','--quiet','HEAD','--']).returncode:
        raise RuntimeError('Hay cambios versionados sin commit. Regístralos antes de preparar fuentes.')
    commit = git('rev-parse','HEAD').decode().strip()
    names = git('ls-tree','-r','--name-only','-z',commit).decode().split('\0')
    check_distributable(names)
    recipe = git('show',f'{commit}:{RECIPES[variant]}').decode()
    version = re.search(r'^pkgver=([\w.]+)$',recipe,re.M).group(1)
    package = re.search(r'^pkgname=([a-z0-9-]+)$',recipe,re.M).group(1)
    prefix=f'{package}-{version}'
    archive=f'{prefix}-{commit[:12]}.tar.gz'
    output.mkdir(parents=True,exist_ok=True)
    destinations=[output/archive,output/'PKGBUILD',output/'source-manifest.json']
    if any(p.exists() for p in destinations):
        raise RuntimeError('El kit ya existe en ese directorio; usa otro destino para conservarlo.')
    created=[]
    try:
        with destinations[0].open('xb') as target:
            created.append(destinations[0])
            excluded=[f':(exclude){p}' for p in DEVELOPMENT_ONLY]
            process=subprocess.Popen(['git','-C',str(ROOT),'archive','--format=tar',f'--prefix={prefix}/',commit,'--','.',*excluded],stdout=subprocess.PIPE)
            try:
                with gzip.GzipFile(filename='',fileobj=target,mode='wb',mtime=0) as zipped:
                    shutil.copyfileobj(process.stdout,zipped)
            finally:
                process.stdout.close()
                status=process.wait()
            if status:
                raise RuntimeError('No se pudo generar el archivo de fuentes Git.')
        digest=hashlib.sha256(destinations[0].read_bytes()).hexdigest()
        recipe=re.sub(r'^source=.*$',f'source=("{archive}")',recipe,flags=re.M)
        recipe=re.sub(r'^sha256sums=.*$',f"sha256sums=('{digest}') # Commit {commit}",recipe,flags=re.M)
        with destinations[1].open('x') as target:
            created.append(destinations[1]);target.write(recipe)
        manifest={'commit':commit,'variant':variant,'version':version,'archive':archive,'sha256':digest,'proprietary_files':False,'excluded_development_paths':list(DEVELOPMENT_ONLY)}
        with destinations[2].open('x') as target:
            created.append(destinations[2]);json.dump(manifest,target,indent=2);target.write('\n')
        print(json.dumps(manifest,indent=2))
    except BaseException:
        for path in created:path.unlink(missing_ok=True)
        raise


if __name__ == '__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output',type=Path,help='Directorio del kit local (sin instalación ni publicación)')
    parser.add_argument('--variant',choices=sorted(RECIPES),default='legacy',help='legacy (Solaar/Python) o native (HID++ propio)')
    arguments=parser.parse_args()
    prepare(arguments.output, arguments.variant)
