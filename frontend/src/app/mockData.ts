import type { GraphNode, GraphEdge, ProjectFile, AnalysisEvent } from './types'

const rnd = (min: number, max: number) => Math.random() * (max - min) + min

export const INITIAL_NODES: GraphNode[] = [
  // External crates (outer ring)
  { id: 'ext-axum', type: 'ExternalCrate', label: 'axum', crate: 'external', x: rnd(-420, -380), y: rnd(-80, 80), vx: 0, vy: 0 },
  { id: 'ext-tokio', type: 'ExternalCrate', label: 'tokio', crate: 'external', x: rnd(380, 420), y: rnd(-80, 80), vx: 0, vy: 0 },
  { id: 'ext-sqlx', type: 'ExternalCrate', label: 'sqlx', crate: 'external', x: rnd(-60, 60), y: rnd(350, 400), vx: 0, vy: 0 },
  { id: 'ext-serde', type: 'ExternalCrate', label: 'serde', crate: 'external', x: rnd(320, 380), y: rnd(250, 300), vx: 0, vy: 0 },
  { id: 'ext-tracing', type: 'ExternalCrate', label: 'tracing', crate: 'external', x: rnd(-380, -320), y: rnd(250, 300), vx: 0, vy: 0 },

  // Crates (middle layer)
  { id: 'crate-server', type: 'Module', label: 'server', crate: 'server', x: rnd(-60, 60), y: rnd(-220, -180), vx: 0, vy: 0 },
  { id: 'crate-auth', type: 'Module', label: 'auth', crate: 'auth', x: rnd(-260, -220), y: rnd(-80, -40), vx: 0, vy: 0 },
  { id: 'crate-db', type: 'Module', label: 'db', crate: 'db', x: rnd(-60, 60), y: rnd(160, 200), vx: 0, vy: 0 },
  { id: 'crate-models', type: 'Module', label: 'models', crate: 'models', x: rnd(220, 260), y: rnd(-80, -40), vx: 0, vy: 0 },

  // Modules
  { id: 'mod-handlers', type: 'Module', label: 'handlers', module: 'handlers', crate: 'server', x: rnd(-140, -100), y: rnd(-120, -80), vx: 0, vy: 0 },
  { id: 'mod-middleware', type: 'Module', label: 'middleware', module: 'middleware', crate: 'server', x: rnd(100, 140), y: rnd(-120, -80), vx: 0, vy: 0 },
  { id: 'mod-jwt', type: 'Module', label: 'jwt', module: 'jwt', crate: 'auth', x: rnd(-300, -260), y: rnd(20, 60), vx: 0, vy: 0 },
  { id: 'mod-queries', type: 'Module', label: 'queries', module: 'queries', crate: 'db', x: rnd(-100, -60), y: rnd(220, 260), vx: 0, vy: 0 },

  // Files
  { id: 'file-main', type: 'File', label: 'main.rs', file: 'src/main.rs', module: 'server', crate: 'server', x: rnd(-40, 40), y: rnd(-300, -260), vx: 0, vy: 0 },
  { id: 'file-users', type: 'File', label: 'users.rs', file: 'src/handlers/users.rs', module: 'handlers', crate: 'server', x: rnd(-200, -160), y: rnd(-200, -160), vx: 0, vy: 0 },
  { id: 'file-posts', type: 'File', label: 'posts.rs', file: 'src/handlers/posts.rs', module: 'handlers', crate: 'server', x: rnd(-80, -40), y: rnd(-180, -140), vx: 0, vy: 0 },
  { id: 'file-auth', type: 'File', label: 'auth.rs', file: 'src/auth.rs', module: 'auth', crate: 'auth', x: rnd(-340, -300), y: rnd(-60, -20), vx: 0, vy: 0 },

  // Structs
  {
    id: 'struct-appstate', type: 'Struct', label: 'AppState',
    file: 'src/main.rs', module: 'server', crate: 'server',
    visibility: 'pub', isGeneric: false,
    signature: 'pub struct AppState { db: Arc<DbPool>, config: Config }',
    x: rnd(60, 100), y: rnd(-80, -40), vx: 0, vy: 0,
  },
  {
    id: 'struct-user', type: 'Struct', label: 'User',
    file: 'src/models/user.rs', module: 'models', crate: 'models',
    visibility: 'pub', isGeneric: false,
    signature: 'pub struct User { pub id: Uuid, pub email: String, pub created_at: DateTime<Utc> }',
    x: rnd(160, 200), y: rnd(0, 40), vx: 0, vy: 0, bookmarked: true,
  },
  {
    id: 'struct-post', type: 'Struct', label: 'Post',
    file: 'src/models/post.rs', module: 'models', crate: 'models',
    visibility: 'pub',
    signature: 'pub struct Post { pub id: Uuid, pub author_id: Uuid, pub content: String }',
    x: rnd(240, 280), y: rnd(60, 100), vx: 0, vy: 0,
  },
  {
    id: 'struct-jwtclaims', type: 'Struct', label: 'JwtClaims',
    file: 'src/auth.rs', module: 'auth', crate: 'auth',
    visibility: 'pub(crate)',
    signature: 'pub(crate) struct JwtClaims { sub: String, exp: usize }',
    x: rnd(-300, -260), y: rnd(60, 100), vx: 0, vy: 0,
  },
  {
    id: 'struct-dbpool', type: 'Struct', label: 'DbPool',
    file: 'src/db/mod.rs', module: 'db', crate: 'db',
    visibility: 'pub',
    signature: 'pub struct DbPool(Pool<Postgres>)',
    x: rnd(-60, -20), y: rnd(280, 320), vx: 0, vy: 0,
  },

  // Enums
  {
    id: 'enum-apperror', type: 'Enum', label: 'AppError',
    file: 'src/error.rs', module: 'server', crate: 'server',
    visibility: 'pub',
    signature: 'pub enum AppError { NotFound, Unauthorized, Internal(String), Db(sqlx::Error) }',
    x: rnd(100, 140), y: rnd(40, 80), vx: 0, vy: 0,
  },
  {
    id: 'enum-permission', type: 'Enum', label: 'Permission',
    file: 'src/auth.rs', module: 'auth', crate: 'auth',
    visibility: 'pub',
    signature: 'pub enum Permission { Read, Write, Admin }',
    x: rnd(-200, -160), y: rnd(60, 100), vx: 0, vy: 0,
  },

  // Traits
  {
    id: 'trait-repository', type: 'Trait', label: 'Repository',
    file: 'src/db/mod.rs', module: 'db', crate: 'db',
    visibility: 'pub', isGeneric: true,
    signature: 'pub trait Repository<T> { async fn find_by_id(&self, id: Uuid) -> Result<T, AppError>; }',
    x: rnd(140, 180), y: rnd(160, 200), vx: 0, vy: 0,
  },
  {
    id: 'trait-authenticatable', type: 'Trait', label: 'Authenticatable',
    file: 'src/auth.rs', module: 'auth', crate: 'auth',
    visibility: 'pub',
    signature: 'pub trait Authenticatable { fn verify_password(&self, password: &str) -> bool; }',
    x: rnd(-180, -140), y: rnd(120, 160), vx: 0, vy: 0,
  },
  {
    id: 'trait-intoresponse', type: 'Trait', label: 'IntoResponse',
    file: 'axum/src/response/mod.rs', module: 'axum', crate: 'external',
    visibility: 'pub',
    signature: 'pub trait IntoResponse { fn into_response(self) -> Response; }',
    x: rnd(-320, -280), y: rnd(-120, -80), vx: 0, vy: 0,
  },

  // Impl blocks
  { id: 'impl-user', type: 'Impl', label: 'impl User', module: 'models', crate: 'models', x: rnd(200, 220), y: rnd(40, 60), vx: 0, vy: 0 },
  { id: 'impl-repo-db', type: 'Impl', label: 'impl Repo for DbPool', module: 'db', crate: 'db', x: rnd(60, 100), y: rnd(200, 240), vx: 0, vy: 0 },
  { id: 'impl-apperror-into', type: 'Impl', label: 'impl IntoResponse for AppError', module: 'server', crate: 'server', x: rnd(-60, -20), y: rnd(60, 100), vx: 0, vy: 0 },

  // Functions
  {
    id: 'fn-create-user', type: 'Function', label: 'create_user',
    file: 'src/handlers/users.rs', module: 'handlers', crate: 'server',
    visibility: 'pub', isAsync: true,
    signature: 'pub async fn create_user(State(state): State<AppState>, Json(body): Json<CreateUserDto>) -> Result<Json<User>, AppError>',
    x: rnd(-180, -140), y: rnd(-220, -180), vx: 0, vy: 0, bookmarked: true,
  },
  {
    id: 'fn-get-user', type: 'Function', label: 'get_user',
    file: 'src/handlers/users.rs', module: 'handlers', crate: 'server',
    visibility: 'pub', isAsync: true,
    signature: 'pub async fn get_user(State(state): State<AppState>, Path(id): Path<Uuid>) -> Result<Json<User>, AppError>',
    x: rnd(-100, -60), y: rnd(-240, -200), vx: 0, vy: 0,
  },
  {
    id: 'fn-authenticate', type: 'Function', label: 'authenticate',
    file: 'src/auth.rs', module: 'auth', crate: 'auth',
    visibility: 'pub', isAsync: true,
    signature: 'pub async fn authenticate(claims: &JwtClaims) -> Result<User, AppError>',
    x: rnd(-260, -220), y: rnd(-40, 0), vx: 0, vy: 0,
  },
  {
    id: 'fn-validate-token', type: 'Function', label: 'validate_token',
    file: 'src/auth/jwt.rs', module: 'jwt', crate: 'auth',
    visibility: 'pub(crate)', isAsync: false,
    signature: 'pub(crate) fn validate_token(token: &str) -> Result<JwtClaims, AppError>',
    x: rnd(-340, -300), y: rnd(20, 60), vx: 0, vy: 0,
  },
  {
    id: 'fn-create-post', type: 'Function', label: 'create_post',
    file: 'src/handlers/posts.rs', module: 'handlers', crate: 'server',
    visibility: 'pub', isAsync: true,
    signature: 'pub async fn create_post(State(state): State<AppState>, Json(body): Json<CreatePostDto>) -> Result<Json<Post>, AppError>',
    x: rnd(-60, -20), y: rnd(-160, -120), vx: 0, vy: 0,
  },

  // Methods
  {
    id: 'method-user-new', type: 'Method', label: 'User::new',
    file: 'src/models/user.rs', module: 'models', crate: 'models',
    signature: 'pub fn new(email: String, password_hash: String) -> Self',
    x: rnd(200, 240), y: rnd(80, 120), vx: 0, vy: 0,
  },
  {
    id: 'method-dbpool-query', type: 'Method', label: 'DbPool::query',
    file: 'src/db/mod.rs', module: 'db', crate: 'db',
    isAsync: true, isGeneric: true,
    signature: 'pub async fn query<T>(&self, sql: &str) -> Result<Vec<T>, sqlx::Error>',
    x: rnd(-40, 0), y: rnd(320, 360), vx: 0, vy: 0,
  },

  // Macros
  {
    id: 'macro-tracing-info', type: 'Macro', label: 'tracing::info!',
    crate: 'tracing',
    x: rnd(-280, -240), y: rnd(160, 200), vx: 0, vy: 0,
  },
  {
    id: 'macro-sqlx-query', type: 'Macro', label: 'sqlx::query!',
    crate: 'sqlx',
    x: rnd(60, 100), y: rnd(280, 320), vx: 0, vy: 0,
  },
]

export const INITIAL_EDGES: GraphEdge[] = [
  // External dependency
  { id: 'e1', source: 'crate-server', target: 'ext-axum', type: 'ExternalDependency' },
  { id: 'e2', source: 'crate-server', target: 'ext-tokio', type: 'ExternalDependency' },
  { id: 'e3', source: 'crate-db', target: 'ext-sqlx', type: 'ExternalDependency' },
  { id: 'e4', source: 'struct-user', target: 'ext-serde', type: 'ExternalDependency' },
  { id: 'e5', source: 'fn-create-user', target: 'macro-tracing-info', type: 'Uses' },

  // Contains (crates -> modules)
  { id: 'e6', source: 'crate-server', target: 'mod-handlers', type: 'Contains' },
  { id: 'e7', source: 'crate-server', target: 'mod-middleware', type: 'Contains' },
  { id: 'e8', source: 'crate-auth', target: 'mod-jwt', type: 'Contains' },
  { id: 'e9', source: 'crate-db', target: 'mod-queries', type: 'Contains' },

  // ModDeclaration (modules -> files)
  { id: 'e10', source: 'mod-handlers', target: 'file-users', type: 'ModDeclaration' },
  { id: 'e11', source: 'mod-handlers', target: 'file-posts', type: 'ModDeclaration' },
  { id: 'e12', source: 'crate-server', target: 'file-main', type: 'ModDeclaration' },
  { id: 'e13', source: 'crate-auth', target: 'file-auth', type: 'ModDeclaration' },

  // Contains (files -> structs/fns)
  { id: 'e14', source: 'file-main', target: 'struct-appstate', type: 'Contains' },
  { id: 'e15', source: 'file-users', target: 'fn-create-user', type: 'Contains' },
  { id: 'e16', source: 'file-users', target: 'fn-get-user', type: 'Contains' },
  { id: 'e17', source: 'file-posts', target: 'fn-create-post', type: 'Contains' },
  { id: 'e18', source: 'file-auth', target: 'fn-authenticate', type: 'Contains' },
  { id: 'e19', source: 'file-auth', target: 'struct-jwtclaims', type: 'Contains' },
  { id: 'e20', source: 'file-auth', target: 'enum-permission', type: 'Contains' },

  // Function calls
  { id: 'e21', source: 'fn-create-user', target: 'method-user-new', type: 'Calls' },
  { id: 'e22', source: 'fn-create-user', target: 'method-dbpool-query', type: 'Calls' },
  { id: 'e23', source: 'fn-get-user', target: 'method-dbpool-query', type: 'Calls' },
  { id: 'e24', source: 'fn-authenticate', target: 'fn-validate-token', type: 'Calls' },
  { id: 'e25', source: 'fn-create-post', target: 'fn-authenticate', type: 'Calls' },
  { id: 'e26', source: 'fn-create-post', target: 'method-dbpool-query', type: 'Calls' },

  // Data flow
  { id: 'e27', source: 'fn-validate-token', target: 'struct-jwtclaims', type: 'DataFlow' },
  { id: 'e28', source: 'fn-authenticate', target: 'struct-user', type: 'DataFlow' },
  { id: 'e29', source: 'fn-create-user', target: 'struct-user', type: 'DataFlow' },
  { id: 'e30', source: 'method-dbpool-query', target: 'struct-dbpool', type: 'DataFlow' },

  // Type references
  { id: 'e31', source: 'struct-appstate', target: 'struct-dbpool', type: 'TypeReference' },
  { id: 'e32', source: 'fn-create-user', target: 'struct-appstate', type: 'TypeReference' },
  { id: 'e33', source: 'fn-create-user', target: 'enum-apperror', type: 'TypeReference' },
  { id: 'e34', source: 'fn-get-user', target: 'enum-apperror', type: 'TypeReference' },
  { id: 'e35', source: 'struct-post', target: 'struct-user', type: 'TypeReference' },

  // Implements
  { id: 'e36', source: 'impl-user', target: 'trait-authenticatable', type: 'Implements' },
  { id: 'e37', source: 'impl-repo-db', target: 'trait-repository', type: 'Implements' },
  { id: 'e38', source: 'impl-apperror-into', target: 'trait-intoresponse', type: 'Implements' },

  // Impl -> target type
  { id: 'e39', source: 'impl-user', target: 'struct-user', type: 'Contains' },
  { id: 'e40', source: 'impl-repo-db', target: 'struct-dbpool', type: 'Contains' },
  { id: 'e41', source: 'impl-apperror-into', target: 'enum-apperror', type: 'Contains' },
  { id: 'e42', source: 'impl-user', target: 'method-user-new', type: 'Contains' },
  { id: 'e43', source: 'impl-repo-db', target: 'method-dbpool-query', type: 'Contains' },

  // Uses
  { id: 'e44', source: 'crate-server', target: 'crate-auth', type: 'Uses' },
  { id: 'e45', source: 'crate-server', target: 'crate-db', type: 'Uses' },
  { id: 'e46', source: 'crate-server', target: 'crate-models', type: 'Uses' },
  { id: 'e47', source: 'crate-auth', target: 'crate-models', type: 'Uses' },
  { id: 'e48', source: 'crate-db', target: 'crate-models', type: 'Uses' },

  // Macro uses
  { id: 'e49', source: 'macro-sqlx-query', target: 'ext-sqlx', type: 'ExternalDependency' },
  { id: 'e50', source: 'fn-create-user', target: 'macro-sqlx-query', type: 'Uses' },
]

export const PROJECT_FILES: ProjectFile[] = [
  { id: 'f1', name: 'main.rs', path: 'server/src/main.rs', module: 'server', crate: 'server', functionsCount: 3, linksCount: 12, diagnosticsCount: 0, complexity: 'medium' },
  { id: 'f2', name: 'users.rs', path: 'server/src/handlers/users.rs', module: 'handlers', crate: 'server', functionsCount: 6, linksCount: 18, diagnosticsCount: 1, complexity: 'high' },
  { id: 'f3', name: 'posts.rs', path: 'server/src/handlers/posts.rs', module: 'handlers', crate: 'server', functionsCount: 4, linksCount: 11, diagnosticsCount: 0, complexity: 'medium' },
  { id: 'f4', name: 'auth.rs', path: 'auth/src/lib.rs', module: 'auth', crate: 'auth', functionsCount: 5, linksCount: 14, diagnosticsCount: 0, complexity: 'high' },
  { id: 'f5', name: 'jwt.rs', path: 'auth/src/jwt.rs', module: 'jwt', crate: 'auth', functionsCount: 3, linksCount: 7, diagnosticsCount: 0, complexity: 'medium' },
  { id: 'f6', name: 'mod.rs', path: 'db/src/mod.rs', module: 'db', crate: 'db', functionsCount: 8, linksCount: 22, diagnosticsCount: 2, complexity: 'high' },
  { id: 'f7', name: 'queries.rs', path: 'db/src/queries.rs', module: 'queries', crate: 'db', functionsCount: 12, linksCount: 30, diagnosticsCount: 0, complexity: 'high' },
  { id: 'f8', name: 'user.rs', path: 'models/src/user.rs', module: 'models', crate: 'models', functionsCount: 4, linksCount: 9, diagnosticsCount: 0, complexity: 'low' },
]

export const ANALYSIS_EVENTS: AnalysisEvent[] = [
  { id: 'ev1', type: 'analyzer', message: 'rust-analyzer indexed 247 files', timestamp: '2s ago' },
  { id: 'ev2', type: 'graph', message: 'Graph snapshot updated — 43 nodes, 50 edges', timestamp: '2s ago' },
  { id: 'ev3', type: 'warning', message: 'Unused variable `_ctx` in handlers/users.rs:84', timestamp: '8s ago', file: 'handlers/users.rs' },
  { id: 'ev4', type: 'warning', message: 'Dead code: `Permission::Admin` never constructed', timestamp: '12s ago', file: 'auth.rs' },
  { id: 'ev5', type: 'analyzer', message: 'File change detected: db/src/queries.rs', timestamp: '34s ago', file: 'db/src/queries.rs' },
  { id: 'ev6', type: 'graph', message: 'Diff applied — 3 edges added, 1 removed', timestamp: '34s ago' },
  { id: 'ev7', type: 'info', message: 'Proc-macro expansion completed (2 macros)', timestamp: '1m ago' },
  { id: 'ev8', type: 'error', message: 'Unresolved import: `crate::config::Settings`', timestamp: '3m ago', file: 'server/src/main.rs' },
]

export const HOTSPOTS = [
  { id: 'hs1', label: 'db/queries.rs', reason: '12 functions, 30 outgoing links', severity: 'high' as const },
  { id: 'hs2', label: 'handlers/users.rs', reason: '6 functions, 18 links, 1 diagnostic', severity: 'high' as const },
  { id: 'hs3', label: 'trait Repository', reason: '3 implementors, 8 usages', severity: 'medium' as const },
  { id: 'hs4', label: 'enum AppError', reason: 'Used in 9 return types', severity: 'medium' as const },
  { id: 'hs5', label: 'struct AppState', reason: 'Passed to 7 handlers', severity: 'low' as const },
]
