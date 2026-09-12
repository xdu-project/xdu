# hpccm

Container specs for xdu, generated with [HPC Container Maker][hpccm].

`xdu.py` is the recipe. `xdu.def` and `xdu.docker` are generated from it and
committed, so change the recipe and regenerate rather than editing them
directly. A handy Makefile is provided to simplify operations.

| file | purpose |
| --- | --- |
| `xdu.py` | the HPCCM recipe |
| `xdu.def` | generated Apptainer/Singularity definition |
| `xdu.docker` | generated Dockerfile |
| `Makefile` | regenerates the specs, builds the images |
| `.gitattributes` | marks the specs as generated |

## Usage

    make            regenerate xdu.def and xdu.docker
    make check      fail unless the specs match the recipe
    make sif        build xdu.sif with apptainer, or singularity
    make image      build the xdu docker image
    make help       every target, and the current variable values

Recipe options — `version`, `prefix`, `runtime_base` — pass through `USERARGS`
verbatim:

    make -B USERARGS='--userarg version=1.2.3 prefix=/usr/local'

The rules key off the recipe name, so adding a second recipe `foo.py` here also
gives you `make foo`, `make foo.def`, `make foo.sif` and `make foo.image`.

## What it builds

A two-stage image holding the four binaries — `xdu`, `xdu-find`, `xdu-view`,
`xdu-rm` — plus man pages and shell completions, taken from the published
release tarball. The download runs in a throwaway first stage, checked against
the published `SHA256SUMS`, keeping `curl` out of the finished image, and the
architecture is resolved inside the
container from `uname -m`, so one spec serves both x86_64 and aarch64.

Generating the Apptainer definition by hand needs `--singularity-version=3.2`,
without which hpccm silently emits only the first stage. The Makefile passes
it as its default, and the recipe refuses to generate below it rather than
letting that pass quietly.

## How this differs from the top-level Dockerfile

The top-level `Dockerfile` **compiles xdu from source**; this recipe
**installs a released binary**. So it needs no Rust toolchain and finishes in
seconds rather than minutes, and it also yields an Apptainer definition — but
it can only install released tags, not an arbitrary commit.

**Why bookworm.** Releases built on the manylinux_2_28 baseline need at most glibc
2.28, so bookworm (2.36) serves as the runtime. The previous Ubuntu 24.04 releases
needed up to 2.39 and ran on trixie instead; the runtime stage still runs `--version`
on all four binaries, so pointing the recipe at a pre-baseline tag fails the build
loudly instead of producing an image that dies on first use.

**Man pages and shell completions.** The release tarball ships them, so this
recipe includes them; the top-level Dockerfile cannot, because `.dockerignore`
keeps `doc/` out of its build context. The image has no `man`, so they are not
very useful unless copied from the container onto the host.

[hpccm]: https://github.com/NVIDIA/hpc-container-maker

## Cutting a new release

The recipe defaults track the latest tag, so a release moves three things together: bump
`VERSION` in `xdu.py`, regenerate both specs with `make -B`, and commit recipe plus specs
as one change. The base-image tags float; before the first container release, digest-pin
them the way the top-level `Dockerfile` header describes.

## Caveats

Container runscript is set to run `xdu`, for the `xdu-***` tools use `exec`
(e.g. `apptainer exec xdu.sif xdu-view`). It is on you to ensure that all
relevant filesystems are bind-mounted into the container (via suitable default
settings and/or explicit `-B` options).

