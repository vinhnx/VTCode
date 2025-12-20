#!/usr/bin/env python3
"""
PDF Text Extractor Script
Extracts text content from PDF files with page numbers.
"""

import sys
import argparse
try:
    import PyPDF2
    import pdfplumber
except ImportError:
    print("ERROR: Required packages not found. Install with:")
    print("pip install PyPDF2 pdfplumber")
    sys.exit(1)


def extract_text_with_pypdf2(pdf_path):
    """Extract text using PyPDF2"""
    try:
        with open(pdf_path, 'rb') as file:
            reader = PyPDF2.PdfReader(file)
            text_by_page = []
            
            for page_num, page in enumerate(reader.pages, 1):
                try:
                    text = page.extract_text()
                    if text and text.strip():
                        text_by_page.append((page_num, text))
                except Exception as e:
                    text_by_page.append((page_num, f"Error extracting text: {e}"))
            
            return text_by_page
    except Exception as e:
        print(f"ERROR: Failed to read PDF {pdf_path}: {e}")
        return None


def extract_text_with_pdfplumber(pdf_path):
    """Extract text using pdfplumber for better accuracy"""
    try:
        with pdfplumber.open(pdf_path) as pdf:
            text_by_page = []
            
            for page_num, page in enumerate(pdf.pages, 1):
                try:
                    text = page.extract_text()
                    if text and text.strip():
                        text_by_page.append((page_num, text))
                except Exception as e:
                    text_by_page.append((page_num, f"Error extracting text: {e}"))
            
            return text_by_page
    except Exception as e:
        print(f"ERROR: Failed to read PDF {pdf_path}: {e}")
        return None


def extract_text(pdf_path, method='pdfplumber'):
    """Extract text from PDF using specified method"""
    print(f"Extracting text from {pdf_path} using {method}...")
    
    if method == 'pdfplumber':
        result = extract_text_with_pdfplumber(pdf_path)
    else:
        result = extract_text_with_pypdf2(pdf_path)
    
    if result is None:
        return False
    
    print(f"\nSuccessfully extracted text from {len(result)} pages:\n")
    print("=" * 60)
    
    for page_num, text in result:
        print(f"\n--- Page {page_num} ---\n")
        print(text[:500])  # Print first 500 chars to avoid overwhelming output
        if len(text) > 500:
            print("... (truncated)")
    
    print("\n" + "=" * 60)
    return True


def main():
    parser = argparse.ArgumentParser(description='Extract text from PDF files')
    parser.add_argument('pdf_path', help='Path to the PDF file')
    parser.add_argument('--method', choices=['pypdf2', 'pdfplumber'], 
                       default='pdfplumber', help='Extraction method')
    parser.add_argument('--output', help='Output file path (optional)')
    
    args = parser.parse_args()
    
    success = extract_text(args.pdf_path, args.method)
    
    if success:
        print(f"\n✅ Text extraction completed successfully!")
        if args.output:
            print(f"Output would be saved to: {args.output}")
    else:
        print(f"\n❌ Text extraction failed!")
        sys.exit(1)


if __name__ == "__main__":
    main()
