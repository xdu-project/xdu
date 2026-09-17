Name:           xdu
Version:        0.5.2
Release:        1%{?dist}
Summary:        High-performance file system indexer for large-scale storage administration
License:        MIT
URL:            https://github.com/xdu-project/xdu
Source0:        %{url}/archive/refs/tags/v%{version}.tar.gz
BuildRequires:       cargo
BuildRequires:       rust
# The duckdb crate's `bundled` feature compiles DuckDB from C++ source; cc-rs
# invokes a tool literally named `c++`, so plain gcc is not enough.
BuildRequires:       gcc-c++

# Disable debug package generation
%global debug_package %{nil}

%description
Extreme-scale parallel "du" command with search and TUI viewer.

%prep
%autosetup -n %{name}-%{version}

%build
cargo build --release --locked \
    --bin xdu --bin xdu-find --bin xdu-view --bin xdu-rm

%install
install -D -m 0755 target/release/xdu %{buildroot}%{_bindir}/xdu
install -D -m 0755 target/release/xdu-find %{buildroot}%{_bindir}/xdu-find
install -D -m 0755 target/release/xdu-view %{buildroot}%{_bindir}/xdu-view
install -D -m 0755 target/release/xdu-rm %{buildroot}%{_bindir}/xdu-rm

# Man pages and shell completions are generated in CI from doc/*.scd and
# src/cli.rs; they are not part of the tag archive yet, so this package ships
# the four binaries only until the release tarball becomes the source.

%files
%{_bindir}/%{name}
%{_bindir}/%{name}-find
%{_bindir}/%{name}-rm
%{_bindir}/%{name}-view

%changelog
* Mon Sep 17 2026 Christopher Orr <chris.orr@gmail.com> - 0.5.2-1
- RPM spec file requires version bump to match release version.
- Disable debug package generation.
* Mon Sep 07 2026 Geoffrey Lentner <glentner@purdue.edu> - 0.4.2-1
- Build from packaged sources; require gcc-c++; correct the license
* Wed Sep  02 2026 Geoffrey Lentner <glentner@purdue.edu> - 0.4.1-1
- First version being packaged

