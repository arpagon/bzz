# source: the disposable profile of the proof reel (PROFILE=here|there)
C=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd); P=/tmp/bzz-demo/${PROFILE:-here}
export BZZ_CONFIG_DIR=$P/config BZZ_DATA_DIR=$P/data BZZ_CACHE_DIR=$P/cache
export PATH=$C/bin/bzz-x86_64-unknown-linux-gnu:$PATH
