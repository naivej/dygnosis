var y a;
var_model(model_name=v,eqtags=['Y']);
@#for j in 1:2
@#if j==2
var_expectation_model(model_name=b,variable=y,auxiliary_model_name=v,horizon=1,discount=a);
@#endif
@#if j==1
change_type(parameters) a;
@#endif
@#endfor
a=.9;
model; [name='Y'] y=.5*y(-1); end;