#!/usr/bin/env python3
"""
File Analyzer Skill for VTCode
Analyzes code files for structure, complexity, and quality metrics
"""

import json
import sys
import os
import argparse
import re
from pathlib import Path
from typing import Dict, List, Any, Optional
from collections import defaultdict

# Optional tree-sitter support
try:
    import tree_sitter
    import tree_sitter_python
    import tree_sitter_javascript
    import tree_sitter_typescript
    import tree_sitter_go
    import tree_sitter_rust
    TREE_SITTER_AVAILABLE = True
except ImportError:
    TREE_SITTER_AVAILABLE = False

class FileAnalyzer:
    """Analyzes code files for various metrics"""
    
    def __init__(self):
        self.languages = {
            'python': {
                'extensions': ['.py'],
                'comment_pattern': r'#.*$',
                'function_pattern': r'def\s+(\w+)\s*\(',
                'class_pattern': r'class\s+(\w+)'
            },
            'javascript': {
                'extensions': ['.js', '.jsx'],
                'comment_pattern': r'//.*$|/\*.*?\*/',
                'function_pattern': r'function\s+(\w+)\s*\(|const\s+(\w+)\s*=\s*(function|.*=>)',
                'class_pattern': r'class\s+(\w+)'
            },
            'typescript': {
                'extensions': ['.ts', '.tsx'],
                'comment_pattern': r'//.*$|/\*.*?\*/',
                'function_pattern': r'function\s+(\w+)\s*\(|const\s+(\w+)\s*=\s*(function|.*=>)',
                'class_pattern': r'class\s+(\w+)'
            },
            'rust': {
                'extensions': ['.rs'],
                'comment_pattern': r'//.*$|/\*.*?\*/',
                'function_pattern': r'fn\s+(\w+)\s*\(',
                'class_pattern': r'struct\s+(\w+)|enum\s+(\w+)|impl\s+(\w+)'
            },
            'go': {
                'extensions': ['.go'],
                'comment_pattern': r'//.*$|/\*.*?\*/',
                'function_pattern': r'func\s+(\w+)\s*\(',
                'class_pattern': r'type\s+(\w+)\s+struct'
            }
        }
    
    def detect_language(self, file_path: str) -> str:
        """Detect programming language from file extension"""
        ext = Path(file_path).suffix.lower()
        for lang, config in self.languages.items():
            if ext in config['extensions']:
                return lang
        return 'unknown'
    
    def analyze_file(self, file_path: str, options: Dict[str, Any]) -> Dict[str, Any]:
        """Analyze a code file and return metrics"""
        if not os.path.exists(file_path):
            raise FileNotFoundError(f"File not found: {file_path}")
        
        # Check file size
        max_size = int(os.environ.get('MAX_FILE_SIZE', 1048576))  # 1MB default
        if os.path.getsize(file_path) > max_size:
            raise ValueError(f"File too large: {os.path.getsize(file_path)} bytes (max: {max_size})")
        
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        # Detect language if not specified
        language = options.get('language', 'auto')
        if language == 'auto':
            language = self.detect_language(file_path)
        
        # Basic metrics
        lines = content.split('\n')
        total_lines = len(lines)
        code_lines = self.count_code_lines(content, language)
        comment_lines = self.count_comment_lines(content, language)
        blank_lines = total_lines - code_lines - comment_lines
        
        # Advanced analysis
        result = {
            "file_path": file_path,
            "language": language,
            "analysis_timestamp": "2024-01-01T00:00:00Z",
            "basic_metrics": {
                "total_lines": total_lines,
                "code_lines": code_lines,
                "comment_lines": comment_lines,
                "blank_lines": blank_lines,
                "file_size_bytes": len(content.encode('utf-8'))
            }
        }
        
        # Perform requested analysis types
        analysis_type = options.get('analysis_type', 'detailed')
        metrics = options.get('metrics', ['complexity', 'structure', 'quality'])
        
        if 'complexity' in metrics:
            result['complexity_metrics'] = self.analyze_complexity(content, language)
        
        if 'structure' in metrics:
            result['structure_metrics'] = self.analyze_structure(content, language)
        
        if 'dependencies' in metrics:
            result['dependency_metrics'] = self.analyze_dependencies(content, language)
        
        if 'quality' in metrics:
            result['quality_metrics'] = self.analyze_quality(content, language, options.get('thresholds', {}))
        
        if analysis_type == 'full':
            result['detailed_analysis'] = self.perform_full_analysis(content, language)
        
        return result
    
    def count_code_lines(self, content: str, language: str) -> int:
        """Count lines of actual code (excluding comments and blanks)"""
        lines = content.split('\n')
        code_lines = 0
        
        for line in lines:
            line = line.strip()
            if line and not self.is_comment_line(line, language):
                code_lines += 1
        
        return code_lines
    
    def count_comment_lines(self, content: str, language: str) -> int:
        """Count comment lines"""
        if language not in self.languages:
            return 0
        
        lines = content.split('\n')
        comment_lines = 0
        comment_pattern = re.compile(self.languages[language]['comment_pattern'], re.MULTILINE)
        
        for line in lines:
            if comment_pattern.match(line.strip()):
                comment_lines += 1
        
        return comment_lines
    
    def is_comment_line(self, line: str, language: str) -> bool:
        """Check if a line is a comment"""
        if language not in self.languages:
            return False
        
        comment_pattern = re.compile(self.languages[language]['comment_pattern'])
        return bool(comment_pattern.match(line.strip()))
    
    def analyze_complexity(self, content: str, language: str) -> Dict[str, Any]:
        """Analyze code complexity metrics"""
        if language not in self.languages:
            return {"error": f"Complexity analysis not supported for language: {language}"}
        
        # Simple cyclomatic complexity estimation
        complexity_indicators = {
            'if': 1,
            'elif': 1,
            'else': 1,
            'for': 1,
            'while': 1,
            'case': 1,
            'catch': 1,
            '&&': 1,
            '||': 1
        }
        
        total_complexity = 0
        for indicator, weight in complexity_indicators.items():
            count = content.lower().count(indicator)
            total_complexity += count * weight
        
        # Count functions/methods
        function_pattern = re.compile(self.languages[language]['function_pattern'])
        functions = function_pattern.findall(content)
        function_count = len(functions)
        
        # Estimate average complexity per function
        avg_complexity = total_complexity / max(function_count, 1)
        
        return {
            "cyclomatic_complexity": total_complexity,
            "function_count": function_count,
            "average_complexity_per_function": round(avg_complexity, 2),
            "max_function_length": self.get_max_function_length(content, language),
            "nesting_depth": self.estimate_nesting_depth(content, language)
        }
    
    def analyze_structure(self, content: str, language: str) -> Dict[str, Any]:
        """Analyze code structure"""
        if language not in self.languages:
            return {"error": f"Structure analysis not supported for language: {language}"}
        
        # Count classes/structs
        class_pattern = re.compile(self.languages[language]['class_pattern'])
        classes = class_pattern.findall(content)
        class_count = len([c for c in classes if c])
        
        # Count functions/methods
        function_pattern = re.compile(self.languages[language]['function_pattern'])
        functions = function_pattern.findall(content)
        function_count = len(functions)
        
        # Analyze file organization
        lines = content.split('\n')
        import_lines = self.count_import_lines(content, language)
        
        return {
            "class_count": class_count,
            "function_count": function_count,
            "import_count": import_lines,
            "functions_per_class": round(function_count / max(class_count, 1), 2),
            "file_organization_score": self.calculate_organization_score(content, language)
        }
    
    def analyze_dependencies(self, content: str, language: str) -> Dict[str, Any]:
        """Analyze dependencies"""
        dependencies = []
        
        # Language-specific dependency patterns
        import_patterns = {
            'python': r'^(?:import|from)\s+([\w.]+)',
            'javascript': r'^(?:import|require)\s*\(?[\'"]([^\'"]+)[\'"]',
            'typescript': r'^(?:import|require)\s*\(?[\'"]([^\'"]+)[\'"]',
            'rust': r'^(?:use|extern\s+crate)\s+([\w:]+)',
            'go': r'^(?:import)\s+([\w.]+)'
        }
        
        if language in import_patterns:
            pattern = re.compile(import_patterns[language], re.MULTILINE)
            dependencies = pattern.findall(content)
        
        # Remove duplicates and sort
        unique_deps = sorted(list(set(dependencies)))
        
        return {
            "dependency_count": len(unique_deps),
            "dependencies": unique_deps[:20],  # Limit to first 20
            "external_dependencies": [dep for dep in unique_deps if '.' in dep or '/' in dep],
            "standard_library_usage": self.count_stdlib_usage(content, language)
        }
    
    def analyze_quality(self, content: str, language: str, thresholds: Dict[str, Any]) -> Dict[str, Any]:
        """Analyze code quality"""
        quality_issues = []
        
        # Calculate metrics
        lines = content.split('\n')
        total_lines = len(lines)
        code_lines = self.count_code_lines(content, language)
        
        # Check function length
        max_function_length = self.get_max_function_length(content, language)
        function_length_threshold = thresholds.get('max_function_length', 50)
        if max_function_length > function_length_threshold:
            quality_issues.append(f"Function length exceeds threshold: {max_function_length} > {function_length_threshold}")
        
        # Check file length
        file_length_threshold = thresholds.get('max_file_length', 500)
        if total_lines > file_length_threshold:
            quality_issues.append(f"File length exceeds threshold: {total_lines} > {file_length_threshold}")
        
        # Check complexity
        if language in self.languages:
            complexity_metrics = self.analyze_complexity(content, language)
            max_complexity = thresholds.get('max_complexity', 10)
            if complexity_metrics.get('cyclomatic_complexity', 0) > max_complexity:
                quality_issues.append(f"Cyclomatic complexity exceeds threshold: {complexity_metrics['cyclomatic_complexity']} > {max_complexity}")
        
        # Calculate quality score
        quality_score = max(0, 100 - len(quality_issues) * 10)
        
        return {
            "quality_score": quality_score,
            "quality_issues": quality_issues,
            "maintainability_index": self.calculate_maintainability_index(content, language),
            "readability_score": self.estimate_readability(content, language)
        }
    
    def perform_full_analysis(self, content: str, language: str) -> Dict[str, Any]:
        """Perform comprehensive analysis"""
        return {
            "detailed_metrics": {
                "character_count": len(content),
                "word_count": len(content.split()),
                "unique_words": len(set(content.lower().split())),
                "comment_density": self.calculate_comment_density(content, language),
                "code_density": self.calculate_code_density(content, language)
            },
            "patterns_detected": self.detect_patterns(content, language),
            "potential_issues": self.detect_potential_issues(content, language),
            "recommendations": self.generate_recommendations(content, language)
        }
    
    # Helper methods
    def get_max_function_length(self, content: str, language: str) -> int:
        """Get the maximum function length"""
        # Simple implementation - would be more sophisticated with proper parsing
        lines = content.split('\n')
        max_length = 0
        current_length = 0
        
        for line in lines:
            if self.is_function_start(line, language):
                current_length = 0
            elif self.is_function_end(line, language):
                max_length = max(max_length, current_length)
                current_length = 0
            else:
                current_length += 1
        
        return max_length
    
    def estimate_nesting_depth(self, content: str, language: str) -> int:
        """Estimate maximum nesting depth"""
        max_depth = 0
        current_depth = 0
        
        # Look for indentation patterns
        lines = content.split('\n')
        for line in lines:
            stripped = line.lstrip()
            if stripped:
                indent_level = len(line) - len(stripped)
                # Simple heuristic: each 4 spaces = 1 level
                depth = indent_level // 4
                max_depth = max(max_depth, depth)
        
        return max_depth
    
    def count_import_lines(self, content: str, language: str) -> int:
        """Count import/include lines"""
        import_patterns = {
            'python': r'^(?:import|from)\s+',
            'javascript': r'^(?:import|require)\s*',
            'typescript': r'^(?:import|require)\s*',
            'rust': r'^(?:use|extern\s+crate)\s+',
            'go': r'^(?:import)\s+'
        }
        
        if language not in import_patterns:
            return 0
        
        pattern = re.compile(import_patterns[language], re.MULTILINE)
        return len(pattern.findall(content))
    
    def calculate_organization_score(self, content: str, language: str) -> float:
        """Calculate file organization score (0-100)"""
        # Simple heuristic based on structure
        score = 50.0  # Base score
        
        # Bonus for having imports at the top
        import_lines = self.count_import_lines(content, language)
        if import_lines > 0:
            score += 10
        
        # Bonus for having comments
        comment_lines = self.count_comment_lines(content, language)
        if comment_lines > 0:
            score += 10
        
        # Penalty for very long functions
        max_function_length = self.get_max_function_length(content, language)
        if max_function_length > 100:
            score -= 20
        
        return max(0, min(100, score))
    
    def count_stdlib_usage(self, content: str, language: str) -> int:
        """Count standard library usage"""
        stdlib_patterns = {
            'python': ['os', 'sys', 'json', 're', 'math', 'datetime'],
            'javascript': ['console', 'Math', 'Date', 'JSON'],
            'rust': ['std', 'core', 'alloc'],
            'go': ['fmt', 'os', 'io', 'net', 'http']
        }
        
        if language not in stdlib_patterns:
            return 0
        
        count = 0
        for module in stdlib_patterns[language]:
            count += content.count(module)
        
        return count
    
    def calculate_maintainability_index(self, content: str, language: str) -> float:
        """Calculate maintainability index (0-100)"""
        # Simplified version of the maintainability index
        lines = content.split('\n')
        total_lines = len(lines)
        code_lines = self.count_code_lines(content, language)
        complexity = self.analyze_complexity(content, language).get('cyclomatic_complexity', 0)
        
        # Simple formula
        mi = 171 - 5.2 * (complexity / max(code_lines, 1)) - 0.23 * total_lines - 16.2 * (self.count_comment_lines(content, language) / max(total_lines, 1))
        
        return max(0, min(100, mi))
    
    def estimate_readability(self, content: str, language: str) -> float:
        """Estimate readability score (0-100)"""
        # Simple heuristic based on various factors
        score = 50.0
        
        # Bonus for comments
        comment_ratio = self.count_comment_lines(content, language) / max(len(content.split('\n')), 1)
        if comment_ratio > 0.1:
            score += 20
        
        # Bonus for short lines
        lines = content.split('\n')
        short_lines = sum(1 for line in lines if len(line) < 80)
        if short_lines / len(lines) > 0.8:
            score += 10
        
        # Penalty for very long functions
        max_function_length = self.get_max_function_length(content, language)
        if max_function_length > 50:
            score -= 15
        
        return max(0, min(100, score))
    
    def calculate_comment_density(self, content: str, language: str) -> float:
        """Calculate comment density (0-1)"""
        total_lines = len(content.split('\n'))
        comment_lines = self.count_comment_lines(content, language)
        return comment_lines / max(total_lines, 1)
    
    def calculate_code_density(self, content: str, language: str) -> float:
        """Calculate code density (0-1)"""
        total_lines = len(content.split('\n'))
        code_lines = self.count_code_lines(content, language)
        return code_lines / max(total_lines, 1)
    
    def detect_patterns(self, content: str, language: str) -> List[str]:
        """Detect common code patterns"""
        patterns = []
        
        # Detect common patterns
        if 'try:' in content and 'except' in content:
            patterns.append('exception_handling')
        
        if 'class ' in content:
            patterns.append('object_oriented')
        
        if 'def ' in content or 'function ' in content:
            patterns.append('functional')
        
        if any(keyword in content for keyword in ['async', 'await', 'Promise']):
            patterns.append('asynchronous')
        
        if 'import ' in content or 'require(' in content:
            patterns.append('modular')
        
        return patterns
    
    def detect_potential_issues(self, content: str, language: str) -> List[str]:
        """Detect potential code issues"""
        issues = []
        
        # Look for common anti-patterns
        if 'eval(' in content:
            issues.append('Use of eval() - security risk')
        
        if 'exec(' in content:
            issues.append('Use of exec() - security risk')
        
        # Check for very long lines
        lines = content.split('\n')
        long_lines = sum(1 for line in lines if len(line) > 120)
        if long_lines > len(lines) * 0.1:
            issues.append(f'Many long lines ({long_lines} lines > 120 chars)')
        
        # Check for deep nesting
        max_depth = self.estimate_nesting_depth(content, language)
        if max_depth > 5:
            issues.append(f'Deep nesting detected (max depth: {max_depth})')
        
        return issues
    
    def generate_recommendations(self, content: str, language: str) -> List[str]:
        """Generate improvement recommendations"""
        recommendations = []
        
        # Comment recommendations
        comment_density = self.calculate_comment_density(content, language)
        if comment_density < 0.05:
            recommendations.append("Consider adding more comments to improve code understanding")
        
        # Function length recommendations
        max_function_length = self.get_max_function_length(content, language)
        if max_function_length > 30:
            recommendations.append("Consider breaking long functions into smaller ones")
        
        # Complexity recommendations
        complexity_metrics = self.analyze_complexity(content, language)
        avg_complexity = complexity_metrics.get('average_complexity_per_function', 0)
        if avg_complexity > 5:
            recommendations.append("Consider reducing function complexity")
        
        # Readability recommendations
        readability = self.estimate_readability(content, language)
        if readability < 70:
            recommendations.append("Consider improving code readability")
        
        return recommendations
    
    def is_function_start(self, line: str, language: str) -> bool:
        """Check if line starts a function"""
        if language not in self.languages:
            return False
        
        function_pattern = re.compile(self.languages[language]['function_pattern'])
        return bool(function_pattern.match(line.strip()))
    
    def is_function_end(self, line: str, language: str) -> bool:
        """Check if line ends a function (simplified)"""
        # This is a very simplified check
        stripped = line.strip()
        return len(stripped) == 0 or stripped.startswith('}') or stripped.startswith('def ') or stripped.startswith('class ')

def main():
    parser = argparse.ArgumentParser(description="File Analyzer Skill for VTCode")
    parser.add_argument("--file", "-f", required=True, help="File to analyze")
    parser.add_argument("--language", "-l", default="auto", help="Programming language")
    parser.add_argument("--analysis-type", "-t", default="detailed", choices=["basic", "detailed", "full"])
    parser.add_argument("--metrics", "-m", nargs="+", default=["complexity", "structure", "quality"],
                        choices=["complexity", "dependencies", "structure", "quality", "duplication"])
    parser.add_argument("--output", "-o", help="Output file")
    parser.add_argument("--verbose", "-v", action="store_true")
    
    args = parser.parse_args()
    
    try:
        analyzer = FileAnalyzer()
        
        # Parse JSON input if provided as first argument
        if args.file.startswith('{'):
            try:
                input_data = json.loads(args.file)
                file_path = input_data.get('file_path', '')
                options = input_data
            except json.JSONDecodeError:
                file_path = args.file
                options = {
                    'language': args.language,
                    'analysis_type': args.analysis_type,
                    'metrics': args.metrics
                }
        else:
            file_path = args.file
            options = {
                'language': args.language,
                'analysis_type': args.analysis_type,
                'metrics': args.metrics
            }
        
        if args.verbose:
            print(f"Analyzing file: {file_path}", file=sys.stderr)
            print(f"Options: {options}", file=sys.stderr)
        
        # Perform analysis
        result = analyzer.analyze_file(file_path, options)
        
        # Add metadata
        result.update({
            "status": "success",
            "skill": "file-analyzer",
            "version": "1.0.0"
        })
        
        # Output results
        if args.output:
            with open(args.output, 'w') as f:
                json.dump(result, f, indent=2)
        else:
            json.dump(result, sys.stdout, indent=2)
            
    except Exception as e:
        error_result = {
            "status": "error",
            "error": str(e),
            "skill": "file-analyzer",
            "version": "1.0.0"
        }
        json.dump(error_result, sys.stderr, indent=2)
        sys.exit(1)

if __name__ == "__main__":
    main()