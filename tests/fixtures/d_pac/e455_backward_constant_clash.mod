// inventory: d_pac_e455_backward_constant_clash
var y x z v;
varexo ey ex ez;
parameters phi psi k beta h_p_constant;
phi=.3; psi=.4; k=.5; beta=.9;
var_model(model_name=aux,eqtags=['Y','X']);
pac_model(model_name=p,auxiliary_model_name=aux,discount=beta);
model;
  [name='Y'] y=phi*y(-1)+ey;
  [name='X'] diff(x)=psi*diff(x(-1))+ex;
  [name='V'] v=x+y;
  [name='P'] diff(z)=k*(v(-1)-z(-1))+psi*diff(z(-1))+pac_expectation(p)+ez;
end;
