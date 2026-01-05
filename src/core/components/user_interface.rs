use anyhow::{Context, Result};
use async_trait::async_trait;
use std::io::{self, Write};

use crate::core::{ProgressBar, ServiceInfo, UserInterface};

pub struct ConsoleUI;

impl ConsoleUI {
    pub fn new() -> Self {
        Self
    }

    fn read_line(&self) -> Result<String> {
        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .context("Failed to read from stdin")?;
        Ok(input.trim().to_string())
    }
}

#[async_trait]
impl UserInterface for ConsoleUI {
    async fn prompt_input(&self, prompt: &str) -> Result<String> {
        print!("{prompt}: ");
        io::stdout().flush().context("Failed to flush stdout")?;
        self.read_line()
    }

    async fn confirm(&self, message: &str) -> Result<bool> {
        loop {
            print!("{message} [y/N]: ");
            io::stdout().flush().context("Failed to flush stdout")?;
            let input = self.read_line()?.to_lowercase();
            if input.is_empty() || input == "n" || input == "no" {
                return Ok(false);
            } else if input == "y" || input == "yes" {
                return Ok(true);
            }
            println!("Please enter 'y' or 'n'");
        }
    }

    async fn select_from_list(&self, items: &[String], prompt: &str) -> Result<usize> {
        for (i, item) in items.iter().enumerate() {
            println!("  {}. {}", i + 1, item);
        }

        loop {
            let input = self
                .prompt_input(&format!("{} [1-{}]", prompt, items.len()))
                .await?;
            if let Ok(idx) = input.parse::<usize>() {
                if idx > 0 && idx <= items.len() {
                    return Ok(idx - 1);
                }
            }
            println!(
                "Invalid selection. Please enter a number between 1 and {}.",
                items.len()
            );
        }
    }

    async fn display_service_table(
        &self,
        _items: &[ServiceInfo],
        _headers: &[&str],
        _formatter: fn(&ServiceInfo) -> Vec<String>,
    ) {
        // Discovery command currently implements its own table display
    }

    async fn show_progress(&self, message: &str) -> Result<Box<dyn ProgressBar>> {
        println!("⏳ {message}...");
        Ok(Box::new(ConsoleProgressBar))
    }
}

pub struct ConsoleProgressBar;

impl ProgressBar for ConsoleProgressBar {
    fn update(&self, _progress: f64) {}

    fn set_message(&self, message: &str) {
        println!("⏳ {message}...");
    }

    fn finish(&self) {}
}
