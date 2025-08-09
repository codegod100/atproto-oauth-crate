use atproto_oauth::Template;

#[derive(Template)]
#[template(path = "home.html")]
pub struct HomeTemplate;

#[derive(Template)]
#[template(path = "success.html")]
pub struct SuccessTemplate {
    pub user_info: Option<UserInfo>,
    pub error_message: Option<String>,
}

#[derive(Template)]
#[template(path = "error.html")]
pub struct ErrorTemplate {
    pub title: String,
    pub handle: Option<String>,
    pub action: Option<String>,
    pub error: String,
}

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