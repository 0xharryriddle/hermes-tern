-- Frozen pre-migration coordination v1 DDL; no current-schema derivation at test time.
CREATE TABLE meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
        CREATE TABLE actors(id TEXT PRIMARY KEY,incarnation TEXT NOT NULL,token_hash TEXT UNIQUE NOT NULL,kind TEXT NOT NULL CHECK(kind IN ('hermes','omp')),profile TEXT NOT NULL,session TEXT NOT NULL,model TEXT NOT NULL,provider TEXT NOT NULL,UNIQUE(kind,profile,session));
        CREATE TABLE routes(sender TEXT REFERENCES actors(id),recipient TEXT REFERENCES actors(id),PRIMARY KEY(sender,recipient));
        CREATE TABLE messages(sequence INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT NOT NULL,sender TEXT NOT NULL REFERENCES actors(id),recipient TEXT NOT NULL REFERENCES actors(id),kind TEXT NOT NULL CHECK(kind IN ('context','task_offer','task_result','question','answer')),body TEXT NOT NULL,body_hash TEXT NOT NULL,created INTEGER NOT NULL CHECK(created>=0),expires INTEGER NOT NULL CHECK(expires>created),state TEXT NOT NULL CHECK(state IN ('queued','submitted','consumed','ambiguous','expired')),delivery_incarnation TEXT,UNIQUE(sender,id));
        CREATE TABLE tasks(id TEXT PRIMARY KEY,owner TEXT NOT NULL REFERENCES actors(id),incarnation TEXT NOT NULL,generation INTEGER NOT NULL CHECK(generation>0),deadline INTEGER NOT NULL,state TEXT NOT NULL CHECK(state IN ('active','released','complete')),result TEXT);
        CREATE TABLE events(sequence INTEGER PRIMARY KEY AUTOINCREMENT,kind TEXT NOT NULL,entity TEXT NOT NULL);
        PRAGMA user_version=1;
