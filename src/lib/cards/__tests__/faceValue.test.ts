import { describe, expect, it } from 'vitest';
import { faceFit, splitFaceValue, textFit } from '../faceValue';

describe('card face value split', () => {
  it('a duration splits into number and unit parts that join back to the original', () => {
    expect(splitFaceValue('8 hr 11 min')).toEqual([{ n: '8' }, { u: 'hr' }, { n: '11' }, { u: 'min' }]);
    expect(splitFaceValue('8h 11m')).toEqual([{ n: '8' }, { u: 'h' }, { n: '11' }, { u: 'm' }]);
  });

  it('decimals, colons and minus signs stay in the number; a separate card unit goes last', () => {
    expect(splitFaceValue('62.5', 'kg')).toEqual([{ n: '62.5' }, { u: 'kg' }]);
    expect(splitFaceValue('4:35', '/km')).toEqual([{ n: '4:35' }, { u: '/km' }]);
    expect(splitFaceValue('-3')).toEqual([{ n: '-3' }]);
  });

  it('a value with no digits is all text; an empty value is an empty array', () => {
    expect(splitFaceValue('Logged')).toEqual([{ u: 'Logged' }]);
    expect(splitFaceValue(null)).toEqual([]);
  });

  it('width estimate: units are smaller, wide characters count more than letters, longer is wider', () => {
    const long = faceFit(splitFaceValue('8 hr 11 min'));
    const short = faceFit(splitFaceValue('8h 11m'));
    expect(long).toBeGreaterThan(short);
    expect(faceFit(splitFaceValue('62'))).toBeLessThan(long);
    expect(faceFit([{ u: '\uff4b'.repeat(4) }])).toBeGreaterThan(faceFit([{ u: 'k'.repeat(4) }]));
    expect(textFit('9/14–9/20')).toBeGreaterThan(textFit('9/16'));
  });
});
