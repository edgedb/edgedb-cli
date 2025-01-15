use crate::print::{self};

pub fn open_link(link: &str, success_prompt: Option<&str>, error_prompt: Option<&str>) -> bool {
    match open::that(link) {
        Ok(_) => {
            print::success_msg("Opening URL in browser", link);
            if let Some(prompt) = success_prompt {
                print::prompt(prompt)
            }
            true
        }
        Err(e) => {
            print::error!("Cannot launch browser: {e:#}");
            print::prompt(error_prompt.unwrap_or("Please paste the URL below into your browser:"));
            println!("{link}");
            false
        }
    }
}
