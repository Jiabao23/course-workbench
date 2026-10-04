"""Offline, reference-gated metrics; standard library only. No model judges itself."""
import argparse
import hashlib
import json
from pathlib import Path
import unicodedata


def normalize(text):
    """NFKC/casefold, remove whitespace and Unicode punctuation; no script conversion."""
    return ''.join(c for c in unicodedata.normalize('NFKC', text).casefold()
                   if not c.isspace() and not unicodedata.category(c).startswith('P'))


def edit_distance(reference, hypothesis):
    # Two rows: bounded auxiliary memory for long-course text.
    if len(reference) < len(hypothesis): reference, hypothesis = hypothesis, reference
    previous = list(range(len(hypothesis) + 1))
    for i, a in enumerate(reference, 1):
        current = [i]
        for j, b in enumerate(hypothesis, 1):
            current.append(min(current[-1]+1, previous[j]+1, previous[j-1]+(a != b)))
        previous = current
    return previous[-1]


def union(intervals):
    clean = []
    for pair in intervals:
        if (not isinstance(pair, (list, tuple)) or len(pair) != 2
                or any(type(v) is not int for v in pair) or not 0 <= pair[0] < pair[1]):
            raise ValueError('Intervals must contain nonnegative integer milliseconds with end > start')
        clean.append(pair)
    merged = []
    for start, end in sorted(clean):
        if merged and start <= merged[-1][1]: merged[-1][1] = max(merged[-1][1], end)
        else: merged.append([start,end])
    return merged


def interval_metrics(reference, predicted):
    truth, guess = union(reference), union(predicted)
    i = j = common = 0
    while i < len(truth) and j < len(guess):
        common += max(0, min(truth[i][1],guess[j][1])-max(truth[i][0],guess[j][0]))
        if truth[i][1] < guess[j][1]: i += 1
        else: j += 1
    truth_ms, guess_ms = sum(b-a for a,b in truth), sum(b-a for a,b in guess)
    return {'reference_ms':truth_ms, 'predicted_ms':guess_ms, 'intersection_ms':common,
            'precision': common/guess_ms if guess_ms else None,
            'recall': common/truth_ms if truth_ms else None,
            'false_positive_ms':guess_ms-common, 'false_negative_ms':truth_ms-common}


def evaluate(reference, hypothesis):
    if (reference.get('schema_version') != 1 or reference.get('reviewed') is not True
            or not str(reference.get('reviewer','')).strip()
            or not str(reference.get('reviewed_at','')).strip()):
        raise ValueError('Quality metrics require an explicitly human-reviewed reference and reviewer/date')
    digest = reference.get('audio_sha256','')
    if len(digest) != 64 or any(c not in '0123456789abcdef' for c in digest) or digest != hypothesis.get('audio_sha256'):
        raise ValueError('Reference and hypothesis must identify the same audio SHA256')
    raw_reference = reference.get('text','')
    raw_hypothesis = '\n'.join(s['text'] for s in hypothesis['segments'])
    clean_reference, clean_hypothesis = normalize(raw_reference), normalize(raw_hypothesis)
    if not clean_reference: raise ValueError('Reference must contain reviewed text')
    terms = []
    for term in reference.get('terms',[]):
        normalized = normalize(term)
        if not normalized: raise ValueError('Term must contain text')
        expected, found = clean_reference.count(normalized), clean_hypothesis.count(normalized)
        if not expected: raise ValueError('Reference term is absent from reviewed text')
        terms.append({'term':term,'reference':expected,'hypothesis':found,
                      'missing':max(0,expected-found),'excess':max(0,found-expected)})
    result = {'schema_version':1, 'audio_sha256':digest,
              'normalization':'NFKC-casefold-no-whitespace-or-punctuation-v1',
              'reference_characters':len(clean_reference),
              'normalized_cer':edit_distance(clean_reference,clean_hypothesis)/len(clean_reference),
              'raw_cer':edit_distance(raw_reference,raw_hypothesis)/len(raw_reference),
              'term_counts':terms, 'reviewer':reference['reviewer'], 'reviewed_at':reference['reviewed_at']}
    if 'speech_intervals_ms' in reference:
        result['speech_coverage'] = interval_metrics(reference['speech_intervals_ms'],
                        [[s['start_ms'],s['end_ms']] for s in hypothesis['segments']])
    if 'omission_intervals_ms' in reference and 'omission_intervals_ms' in hypothesis:
        result['omission_detection'] = interval_metrics(reference['omission_intervals_ms'], hypothesis['omission_intervals_ms'])
    return result


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--audio',type=Path,required=True)
    parser.add_argument('--result',type=Path,required=True,help='worker done JSON (not JSON Lines)')
    parser.add_argument('--reference',type=Path)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    digest=hashlib.file_digest(args.audio.open('rb'),'sha256').hexdigest()
    result=json.loads(args.result.read_text(encoding='utf-8-sig'))
    if result.get('audio_sha256') != digest:
        parser.error('Worker result does not match --audio SHA256')
    report={'schema_version':1,'audio_sha256':digest,'quality_status':'not_measured',
            'performance':{key:result.get(key) for key in (
                'engine','engine_version','model','model_sha256','device','compute_type','decode_options',
                'elapsed_seconds','process_seconds','timings','peak_ram_mb','peak_gpu_mb','gpu_metric',
                'resumed_chunks','checkpoint_key','manifest')}}
    if args.reference:
        try: report['quality']=evaluate(json.loads(args.reference.read_text(encoding='utf-8-sig')),result)
        except ValueError as error: parser.error(str(error))
        report['quality_status']='human_reference_measured'
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(json.dumps(report,ensure_ascii=False,indent=2,allow_nan=False),encoding='utf-8')


if __name__=='__main__': main()
