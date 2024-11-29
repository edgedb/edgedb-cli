use crate::print::{self};

pub fn open_link(link: &str, success_prompt: Option<&str>, error_prompt: Option<&str>) -> bool {
    match open::that(&link) {
        Ok(_) => {
            print::success_msg("Opening URL in browser", link);
            match success_prompt {
                Some(prompt) => print::prompt(prompt),
                None => (),
            }
            true
        }
        Err(e) => {
            print::error!("Cannot launch browser: {e:#}");
            print::prompt(match error_prompt {
                Some(prompt) => prompt,
                None => "Please paste the URL below into your browser:",
            });
            println!("{link}");
            false
        }
    }
}
