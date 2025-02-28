use pdfium_render::prelude::*;
use regex::Regex;
use serde::Serialize;
use std::{env, fs, path::Path};
use chrono::NaiveDate;
use plotters::prelude::*;
use std::collections::BTreeMap;

#[derive(Serialize)]
struct ReceiptResult {
    filename: String,
    date: Option<String>,
    total: Option<f64>,
    error: Option<String>,
}

fn extract_text_from_pdf(pdf_path: &Path) -> Result<String, String> {
    let pdfium = Pdfium::default();
    let document = pdfium.load_pdf_from_file(pdf_path, None).map_err(|e| e.to_string())?;
    let mut full_text = String::new();

    for page_index in 0..document.pages().len() {
        let page = document.pages().get(page_index).map_err(|e| e.to_string())?;
        full_text.push_str(&page.text().map_err(|e| e.to_string())?.to_string());

    }
    Ok(full_text)
}

fn parse_total(text: &str) -> Option<f64> {
    let total_regex = Regex::new(r"(?i)TOTAL A PAGAR\s*\$?(\d+\,\d{2})").unwrap();
    if let Some(captures) = total_regex.captures(text) {
        let total_match = captures.get(0).unwrap().as_str();
        let parsed_total = captures.get(1).and_then(|m| m.as_str().replace(",", ".").parse::<f64>().ok());

        println!("Match found: {}", total_match);
        if let Some(parsed_total) = parsed_total {
            println!("Parsed f64: {}", parsed_total);
        } else {
            println!("Failed to parse f64");
        }

        parsed_total
    } else {
        None
    }
}

fn process_receipts(dir: &str) -> Vec<ReceiptResult> {
    let mut results = Vec::new();

    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().unwrap_or_default() == "pdf" {
            let filename = path.file_name().unwrap().to_string_lossy().to_string();
            println!("Processing file: {}", filename);
            // get the date from the filename
            // e.g. Fatura_Cartao_Continente_20181223_1509.pdf -> 2018-12-23
            let date_regex = Regex::new(r"_(\d{4})(\d{2})(\d{2})_\d{4}\.pdf").unwrap();
            let date = date_regex.captures(&filename).map(|captures| {
                format!("{}-{}-{}", captures.get(1).unwrap().as_str(), captures.get(2).unwrap().as_str(), captures.get(3).unwrap().as_str())
            });
            match extract_text_from_pdf(&path) {
                Ok(text) => {
                    let total = parse_total(&text);
                    results.push(ReceiptResult {
                        filename,
                        date,
                        total,
                        error: None,
                    });
                }
                Err(e) => {
                    results.push(ReceiptResult {
                        filename,
                        date,
                        total: None,
                        error: Some(e),
                    });
                }
            }
        }
    }

    results
}

fn calculate_total(results: &[ReceiptResult]) -> f64 {
    results.iter().filter_map(|r| r.total).sum()
}

fn create_monthly_graph(results: &[ReceiptResult], output_file: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Create a BTreeMap to store monthly totals (automatically sorted by key)
    let mut monthly_totals: BTreeMap<String, f64> = BTreeMap::new();

    // Aggregate totals by month
    for receipt in results {
        if let (Some(date_str), Some(total)) = (&receipt.date, receipt.total) {
            if let Ok(date) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
                let month_key = format!("{}-{:02}", date.format("%Y"), date.format("%m"));
                *monthly_totals.entry(month_key).or_insert(0.0) += total;
            }
        }
    }
    
    // Extract month labels and values
    let month_labels: Vec<String> = monthly_totals.keys().cloned().collect();
    let month_values: Vec<f64> = monthly_totals.values().cloned().collect();
    let max_value = month_values.iter().fold(0.0 as f64, |max , &val| max.max(val));

    // Create the graph
    let root = BitMapBackend::new(output_file, (1000, 600)).into_drawing_area();
    root.fill(&WHITE)?;

    let mut chart = ChartBuilder::on(&root)
        .caption("Monthly Spending", ("sans-serif", 50).into_font())
        .margin(5)
        .x_label_area_size(60)
        .y_label_area_size(60)
        .build_cartesian_2d(
            0..monthly_totals.len(),
            0f64..(max_value * 1.1),
        )?;

    chart
        .configure_mesh()
        .disable_x_mesh()
        .bold_line_style(&WHITE.mix(0.3))
        .y_desc("Amount (€)")
        .x_desc("Month")
        .axis_desc_style(("sans-serif", 15))
        .x_labels(monthly_totals.len()) // Set label count
        .x_label_formatter(&|idx| {
            // Show only every 6th month to avoid crowding
            if *idx < month_labels.len() && *idx % 6 == 0 {
                return month_labels[*idx].clone();
            }
            String::new() // Empty string for non-labeled months
        })
        .draw()?;

    // Draw the bars
    chart.draw_series(
        monthly_totals
            .values()
            .enumerate()
            .map(|(i, &v)| {
                // Color gradient from light blue to dark blue based on value
                let color_intensity = (v / max_value).min(1.0) as f64;
                let color = RGBColor(0, 
                                     (150.0 * (1.0 - color_intensity)).round() as u8, 
                                     (255.0 - (100.0 * color_intensity)).round() as u8);
                
                Rectangle::new(
                    [(i, 0.0), (i + 1, v)],
                    color.filled(),
                )
            })
    )?;

    // Draw the 3-month smooth moving average
    let mut moving_avg_3: Vec<f64> = Vec::new();
    let mut moving_avg_12: Vec<f64> = Vec::new();
    for i in 0..month_values.len() {
        let start = i.saturating_sub(1);
        let end_3 = (i + 2).min(month_values.len());
        let end_12 = (i + 11).min(month_values.len());
        let avg_3 = month_values[start..end_3].iter().sum::<f64>() / (end_3 - start) as f64;
        let avg_12 = month_values[start..end_12].iter().sum::<f64>() / (end_12 - start) as f64;
        moving_avg_3.push(avg_3);
        moving_avg_12.push(avg_12);
    }
    chart.draw_series(LineSeries::new(
        moving_avg_3.iter().enumerate().map(|(i, &v)| (i, v)),
        &RED,
    ))?;
    chart.draw_series(LineSeries::new(
        moving_avg_12.iter().enumerate().map(|(i, &v)| (i, v)),
        &GREEN,
    ))?;


    root.present()?;
    Ok(())
}

#[tokio::main]
async fn main() {
    let receipts_dir = "receipts";
    let output_path = env::var("OUTPUT_PATH").unwrap_or("results".to_string());
    let output_results = output_path.clone() + "/results.json";

    let results = process_receipts(receipts_dir);

    fs::write(output_results.clone(), serde_json::to_string_pretty(&results).unwrap()).unwrap();
    println!("Results written to {}", output_results);

    let total = calculate_total(&results);
    
    // Create and save the monthly spending graph
    if let Err(e) = create_monthly_graph(&results, &(output_path+"/monthly_spending.png")) {
        eprintln!("Error creating graph: {}", e);
    } else {
        println!("Graph saved as monthly_spending.png");
    }

    println!("Total: {}", total);
}
