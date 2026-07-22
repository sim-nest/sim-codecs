# Load a native codec script

The loadable Lisp codec exposes a CLI entrypoint that can decode and evaluate
source supplied by the host. This recipe sends source through that entrypoint
without touching the filesystem and checks that the decoded source returns the
expected symbol.
