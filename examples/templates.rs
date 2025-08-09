// BlogDeleteTemplate removed (inline JS confirm + direct POST used instead)
use atproto_oauth::Template;

#[derive(Template)]
#[template(path = "home.html", config = "examples/askama.toml")]
pub struct HomeTemplate;

#[derive(Template)]
#[template(path = "success.html", config = "examples/askama.toml")]
pub struct SuccessTemplate {
    pub user_info: Option<UserInfo>,
    pub error_message: Option<String>,
}

#[derive(Template)]
#[template(path = "error.html", config = "examples/askama.toml")]
pub struct ErrorTemplate {
    pub title: String,
    pub handle: Option<String>,
    pub action: Option<String>,
    pub error: String,
}

#[derive(Template)]
#[template(path = "blog_list.html", config = "examples/askama.toml")]
pub struct BlogListTemplate {
    pub posts: Vec<BlogPostInfo>,
    pub success_message: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Template)]
#[template(path = "blog_create.html", config = "examples/askama.toml")]
pub struct BlogCreateTemplate;

#[derive(Template)]
#[template(path = "blog_edit.html", config = "examples/askama.toml")]
pub struct BlogEditTemplate {
    pub post: BlogPostInfo,
}

#[derive(Template)]
#[template(path = "blog_view.html", config = "examples/askama.toml")]
pub struct BlogViewTemplate {
    pub post: BlogPostInfo,
}


// TestTemplate removed

#[derive(Debug)]
pub struct UserInfo {
    pub handle: Option<String>,
    pub display_name: Option<String>,
    pub did: Option<String>,
    pub followers_count: Option<u32>,
    pub follows_count: Option<u32>,
    pub posts_count: Option<u32>,
    pub description: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BlogPostInfo {
    pub uri: String,
    pub title: String,
    pub content: String,
    pub summary: Option<String>,
    pub tags: String, // JSON serialized array
    pub formatted_tags: String, // human editable comma list (no brackets) for form
    pub published: bool,
    pub created_at: String, // RFC3339 formatted
    pub updated_at: String, // RFC3339 formatted
}