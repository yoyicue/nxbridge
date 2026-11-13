#!/usr/bin/env python3
"""Exercise actual cache path/cleanup functions without submitting GPU work.

The pinned original upstream is a negative control for both regressions.
"""
import argparse
from pathlib import Path
import subprocess
import tempfile


def function(source, name):
    lines = source.splitlines()
    start = next(i for i, line in enumerate(lines) if line.startswith('static ') and name + '(' in line and '{' in line)
    end = next(i for i in range(start + 1, len(lines)) if lines[i] == '}')
    return '\n'.join(lines[start:end + 1])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--engine', type=Path, required=True)
    parser.add_argument('--case', choices=('paths', 'cleanup'), default='paths')
    args = parser.parse_args()
    directory = args.engine / 'graphics/nvn/gfx/nvn'
    prefix = '''#import <Foundation/Foundation.h>
#include <stdatomic.h>
#include <stdlib.h>
#include <stdio.h>
#define MLOG(...) ((void)0)
static int checks;
#define CHECK(x) do { checks++; if (!(x)) { fprintf(stderr, "FAIL: %s\\n", #x); return 1; } } while (0)
'''
    if args.case == 'paths':
        body = prefix + '''static NSString *map, *user;
static NSString *shmap_path(void) { return map; }
static NSString *pso_user_path(void) { return user; }
'''+ function((directory / 'nvn_mtl.m').read_text(), 'warm_paths') + '''
int main(int argc, char **argv) { @autoreleasepool {
    NSString *root = @(argv[1]); map = [root stringByAppendingPathComponent:@"shader_map.tsv"];
    user = [root stringByAppendingPathComponent:@"pso_user.tsv"];
    NSString *seed = [root stringByAppendingPathComponent:@"pso_manifest.tsv"];
    CHECK(warm_paths(1) == nil);
    [@"S\\tfixture\\t0\\n" writeToFile:user atomically:YES encoding:NSUTF8StringEncoding error:nil];
    CHECK([warm_paths(1) isEqualToArray:@[user]]);
    [@"S\\tfixture\\t0\\n" writeToFile:seed atomically:YES encoding:NSUTF8StringEncoding error:nil];
    CHECK(([warm_paths(1) isEqualToArray:@[seed, user]]));
    [NSFileManager.defaultManager removeItemAtPath:user error:nil];
    CHECK([warm_paths(1) isEqualToArray:@[seed]]);
    setenv("DM_PSO_MANIFEST", "off", 1); CHECK(warm_paths(1) == nil);
    setenv("DM_PSO_MANIFEST", "/explicit/manifest", 1);
    CHECK([warm_paths(1) isEqualToArray:@[@"/explicit/manifest"]]);
    printf("prewarm paths: %d checks passed\\n", checks); return 0;
} }
'''
    else:
        body = prefix + '''static NSString *own, *shared;
static _Atomic uint64_t g_par_ga_removed;
static NSSet *par_ga_list(int own_only) { return own_only ? [NSSet setWithObject:own] : [NSSet setWithArray:@[own, shared]]; }
''' + function((directory / 'pso_archive.h').read_text(), 'par_ga_remove_new') + '''
int main(int argc, char **argv) { @autoreleasepool {
    NSString *root = @(argv[1]); own = [root stringByAppendingPathComponent:@"own"];
    shared = [root stringByAppendingPathComponent:@"other-application"];
    [@"ours" writeToFile:own atomically:YES encoding:NSUTF8StringEncoding error:nil];
    [@"retain" writeToFile:shared atomically:YES encoding:NSUTF8StringEncoding error:nil];
    par_ga_remove_new([NSSet set]);
    CHECK([NSFileManager.defaultManager fileExistsAtPath:shared]);
    CHECK(![NSFileManager.defaultManager fileExistsAtPath:own]);
    CHECK(atomic_load(&g_par_ga_removed) == 1);
    printf("archive cleanup isolation: %d checks passed\\n", checks); return 0;
} }
'''
    with tempfile.TemporaryDirectory(prefix='nxbridge-native-cache-') as tmp:
        root = Path(tmp); source = root / 'test.m'; source.write_text(body)
        subprocess.run(['clang', '-fobjc-arc', '-Wno-incompatible-pointer-types',
                        '-framework', 'Foundation', str(source), '-o', str(root / 'test')], check=True)
        return subprocess.run([str(root / 'test'), str(root)], timeout=5).returncode


if __name__ == '__main__': raise SystemExit(main())
