var y;
@#for i in 1:2
@#if i == 2
pac_model(model_name=q,discount=z);
@#endif
@#if i == 1
parameters z;
@#endif
@#endfor
model; y=z; end;