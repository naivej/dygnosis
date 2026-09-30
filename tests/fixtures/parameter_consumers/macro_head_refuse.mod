var y; varexo z;
@#for i in 1:2
std(z).prior(shape=inv_gamma,mean=.1,stdev=.2);
change_type(parameters) z;
@#endfor
model; y=z; end;