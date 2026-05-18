# HTTP Router

This example builds an HTTP path router using `RegexSet` for fast route classification and individual `Regex` patterns for parameter extraction.

## The design

HTTP routers need to:
1. Match an incoming path against many route patterns
2. Determine which route matched
3. Extract path parameters (`:id`, `:name`, etc.)

`RegexSet` handles steps 1 and 2 simultaneously, and individual captures handle step 3.

## Basic router

```rust
# use rgx_core::{Regex, RegexSet};
struct Route {
    pattern: Regex,
    handler_name: &'static str,
}

struct Router {
    set: RegexSet,
    routes: Vec<Route>,
}

impl Router {
    fn new(route_defs: Vec<(&str, &'static str)>) -> Self {
        let patterns: Vec<String> = route_defs.iter()
            .map(|(pat, _)| format!("^{pat}$"))
            .collect();

        let set = RegexSet::new(&patterns).unwrap();

        let routes: Vec<Route> = route_defs.into_iter()
            .map(|(pat, handler_name)| Route {
                pattern: Regex::compile(&format!("^{pat}$")).unwrap(),
                handler_name,
            })
            .collect();

        Router { set, routes }
    }

    fn route(&self, path: &str) -> Option<(&str, Vec<(&str, String)>)> {
        let matches = self.set.matches(path);

        // Take the first matching route
        for idx in matches.iter() {
            let route = &self.routes[idx];
            if let Some(caps) = route.pattern.captures(path) {
                // Extract named parameters
                let params: Vec<(&str, String)> = route.pattern
                    .capture_names()
                    .enumerate()
                    .skip(1)  // skip group 0
                    .filter_map(|(i, name)| {
                        name.and_then(|n| {
                            caps.get(i).map(|m| (n, m.as_str().to_string()))
                        })
                    })
                    .collect();

                return Some((route.handler_name, params));
            }
        }

        None
    }
}

let router = Router::new(vec![
    (r"/users",                          "list_users"),
    (r"/users/(?P<id>\d+)",              "get_user"),
    (r"/users/(?P<id>\d+)/posts",        "user_posts"),
    (r"/posts/(?P<slug>[\w-]+)",         "get_post"),
    (r"/api/v(?P<version>\d+)/(?P<resource>\w+)", "api_resource"),
]);

// Route: /users
let (handler, params) = router.route("/users").unwrap();
assert_eq!(handler, "list_users");
assert!(params.is_empty());

// Route: /users/42
let (handler, params) = router.route("/users/42").unwrap();
assert_eq!(handler, "get_user");
assert_eq!(params[0], ("id", "42".to_string()));

// Route: /users/42/posts
let (handler, params) = router.route("/users/42/posts").unwrap();
assert_eq!(handler, "user_posts");
assert_eq!(params[0], ("id", "42".to_string()));

// Route: /posts/hello-world
let (handler, params) = router.route("/posts/hello-world").unwrap();
assert_eq!(handler, "get_post");
assert_eq!(params[0], ("slug", "hello-world".to_string()));

// Route: /api/v2/widgets
let (handler, params) = router.route("/api/v2/widgets").unwrap();
assert_eq!(handler, "api_resource");
assert_eq!(params[0], ("version", "2".to_string()));
assert_eq!(params[1], ("resource", "widgets".to_string()));

// No match
assert!(router.route("/unknown/path").is_none());
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Adding HTTP method dispatch

Extend the router to match on both method and path:

```rust
# use rgx_core::{Regex, RegexSet};
struct MethodRoute {
    method: &'static str,
    pattern: Regex,
    handler_name: &'static str,
}

struct MethodRouter {
    set: RegexSet,
    routes: Vec<MethodRoute>,
}

impl MethodRouter {
    fn new(defs: Vec<(&'static str, &str, &'static str)>) -> Self {
        let patterns: Vec<String> = defs.iter()
            .map(|(method, path, _)| format!("^{method} {path}$"))
            .collect();

        let set = RegexSet::new(&patterns).unwrap();

        let routes = defs.into_iter()
            .map(|(method, path, handler)| MethodRoute {
                method,
                pattern: Regex::compile(&format!("^{method} {path}$")).unwrap(),
                handler_name: handler,
            })
            .collect();

        MethodRouter { set, routes }
    }

    fn route(&self, method: &str, path: &str) -> Option<&str> {
        let input = format!("{method} {path}");
        let matches = self.set.matches(&input);
        matches.iter().next().map(|idx| self.routes[idx].handler_name)
    }
}

let router = MethodRouter::new(vec![
    ("GET",    r"/users",            "list_users"),
    ("POST",   r"/users",            "create_user"),
    ("GET",    r"/users/\d+",        "get_user"),
    ("PUT",    r"/users/\d+",        "update_user"),
    ("DELETE", r"/users/\d+",        "delete_user"),
]);

assert_eq!(router.route("GET", "/users"), Some("list_users"));
assert_eq!(router.route("POST", "/users"), Some("create_user"));
assert_eq!(router.route("DELETE", "/users/5"), Some("delete_user"));
assert_eq!(router.route("PATCH", "/users"), None);
```

## Middleware-style processing with callbacks

Use native callbacks to add validation during route matching:

```rust
# use rgx_core::{Regex, ExecutionMode, ExecResult};
let re = Regex::with_mode(
    r"^/api/v(?P<version>\d+)/(?P<resource>\w+)(?{native:validate_version})$",
    ExecutionMode::Full,
)?;

re.set_var("max_api_version", 3_i64)?;

re.register_native("validate_version", |ctx| {
    let version: i64 = ctx.named("version")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let max = ctx.var_int("max_api_version").unwrap_or(1);

    if version >= 1 && version <= max {
        ExecResult::Success
    } else {
        ExecResult::Failure
    }
})?;

assert!(re.is_match("/api/v2/users"));   // version 2 is valid
assert!(!re.is_match("/api/v5/users"));  // version 5 exceeds max
assert!(!re.is_match("/api/v0/users"));  // version 0 is invalid
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Performance considerations

- `RegexSet::matches` tests all patterns in a single pass, which is faster than testing N patterns individually
- For a small number of routes (< 50), the overhead is negligible
- For large route tables, the set-based approach scales better than linear scan
- Compile patterns once at startup and reuse them for every request
- Named captures add minimal overhead -- use them freely for readability

## Key takeaways

- `RegexSet` answers "which routes match?" in one call
- Individual `Regex::captures` extracts path parameters from the winning route
- Named groups (`?P<id>`) make parameter extraction self-documenting
- Native callbacks can add validation logic (API versioning, auth checks) inline
- The pattern compiles once; routing is just matching against compiled patterns
