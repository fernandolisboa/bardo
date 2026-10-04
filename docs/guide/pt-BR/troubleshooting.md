---
id: troubleshooting
title: Solução de problemas
group: reference
---

# Solução de problemas

O que as mensagens do Bardo querem dizer quando algo para, e o que fazer. Uma tarefa que parou mostra o motivo no cartão dela em [Tarefas](jobs.md#retry), com **Detalhes**; depois de resolver a causa, **Tentar de novo** continua de onde parou.

<a id="missing-key"></a>
## Falta uma chave

"Salve uma chave … em Configurações" quer dizer que a etapa precisa de um provedor cuja chave ainda não foi salva. Salve em [Configurações › Chaves de API](bardo:go/settings/keys) e faça a etapa de novo, ou use **Tentar de novo** na tarefa dela. "O provedor recusou a chave" quer dizer que ela foi salva, mas está errada, revogada ou sem uma permissão: **Testar chave** diz qual, e [Testar uma chave](api-keys.md#testing) explica cada resposta.

<a id="budget"></a>
## Um orçamento foi atingido

**Orçamento atingido** antes de uma geração quer dizer que ela levaria um provedor além do orçamento mensal que você definiu, ou que o orçamento já foi usado. **Gerar mesmo assim** segue só com esta geração; **Cancelar** desiste dela. Para não ser mais perguntado, aumente ou remova o orçamento em [Custos](costs.md#budgets). Nada é interrompido no meio por causa de um orçamento.

<a id="quota"></a>
## Uma cota acabou

"Uma cota ou limite de uso parou o provedor" quer dizer que o provedor está recusando chamadas por enquanto, não que algo quebrou. As cotas do YouTube, da Data API e de envios, renovam todo dia à meia-noite do horário do Pacífico; os resultados de pesquisa ficam guardados por sete dias, então rodar de novo dentro de uma semana não gasta nada. Um provedor que funciona com créditos para quando eles acabam: adicione crédito no painel dele. Tarefas paradas por limite de requisições tentam de novo sozinhas primeiro; quando desistem, use **Tentar de novo** mais tarde.

<a id="reconnect"></a>
## Uma conta precisa reconectar

**Reconexão necessária** numa conta quer dizer que a rede recusou renovar o acesso do Bardo: você revogou, ele expirou (um cliente do Google em status de Teste dura sete dias, um login do TikTok um ano sem uso) ou, no Instagram, a conta não está mais ligada a uma Página que você gerencia. Até você reconectar, os envios para ela esperam e as sincronizações pulam os números de dono. Use **Reconectar** no cartão dela em [Contas](network-accounts.md#states); no Instagram, gere um token novo antes.

<a id="upload-limit"></a>
## Um envio está segurado por um limite

**Acima do limite de publicação** quer dizer que a rede só aceita um tanto de posts de apps por dia: o Instagram conta posts em 24 horas, o TikTok rascunhos em 24 horas, e o YouTube tem uma cota diária de envios por projeto do Google e um limite de envios por canal. O envio fica na fila, o cartão diz quando sai, e o Bardo manda sozinho nessa hora (deixe o Bardo aberto). Posts enviados por outros apps também contam, então o Bardo lê o limite de novo antes de tentar. Veja [Estados do envio](uploading.md#states).

<a id="out-of-date"></a>
## Um render ou uma exportação está desatualizado

**Desatualizado** no render de uma rede quer dizer que o corte ou o preset da conta mudou depois que o arquivo foi feito; renderize de novo, e a revisão já marca essa rede ([O último arquivo](render.md#last)). **Desatualizada** numa exportação quer dizer que o render ou os metadados mudaram depois dela; exporte de novo ([Desatualizada](exporting.md#outdated)). As etapas anteriores marcam do mesmo jeito o que foi feito a partir delas: uma narração depois que o roteiro mudou, as cenas depois que a narração mudou.

<a id="logs"></a>
## Ainda sem solução

O log do Bardo, em `%LOCALAPPDATA%\Bardo\logs\bardo.log`, registra cada passo sem nenhuma chave ou token ([Logs](data-and-keys.md#logs)). Os **Detalhes** de uma tarefa com falha mostram a mensagem do próprio provedor, que a documentação dele explica.
