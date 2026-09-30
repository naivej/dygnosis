var y;
@#for i in 1:2
@#if i == 2
z.subsamples(s=2000Q1:2000Q4);
@#endif
@#if i == 1
parameters z;
@#endif
@#endfor
model; y=z; end;