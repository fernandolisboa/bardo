---
id: jobs
title: Tarefas
group: reference
place: jobs
tour: jobs
---

# Tarefas

Trabalho longo roda como **tarefa**: pesquisa, roteiro, narração, imagens, clipes, render, exportação, envio, sincronização de métricas. As tarefas rodam em segundo plano, então você continua trabalhando enquanto elas andam, e elas sobrevivem ao Bardo fechar. [Mostre o painel](bardo:tour/jobs).

<a id="panel"></a>
## O painel Tarefas

**Tarefas** na navegação abre um painel ao lado da tela em que você está, e fecha de novo; a linha ao lado diz quantas tarefas estão rodando ou esperando. O painel agrupa as tarefas em **Em execução**, **Na fila**, **Com falha** e **Encerradas**, cada cartão com o que a tarefa faz e onde ela está. Uma tarefa que espera a hora dela, como um post agendado, diz quando sai.

<a id="progress"></a>
## Progresso

Uma tarefa em execução mostra uma barra e o percentual. As telas também mostram as próprias tarefas: uma etapa diz que está gerando, e o resultado aparece quando a tarefa termina, sem precisar olhar o painel.

<a id="cancel"></a>
## Cancelar

**Cancelar** para uma tarefa em execução ou na fila. O que ela já fez fica guardado para uma nova tentativa, e uma chamada que o provedor já respondeu está paga e conta em [Custos](costs.md#spent). Um render ou uma exportação cancelada guarda as redes que já terminou, e uma nova tentativa faz as que faltam.

<a id="retry"></a>
## Tentar de novo

Quando um provedor falha por um motivo passageiro (tempo esgotado, servidor ocupado, limite de requisições), o Bardo tenta de novo sozinho, até quatro tentativas no total, esperando mais a cada vez, e o cartão diz qual tentativa falhou. Quando a falha é duradoura, como uma chave que falta ou um login recusado, a tarefa para como **Com falha**, com o que deu errado e **Detalhes**.

**Tentar de novo**, numa tarefa com falha ou cancelada, coloca ela na fila de novo. Ela continua do último ponto que salvou, não do começo. Resolva antes o que a falha aponta: veja [Solução de problemas](troubleshooting.md).

<a id="restart"></a>
## Fechar o Bardo no meio de uma tarefa

Fechar o Bardo, ou desligar o computador, nunca perde uma tarefa. Na próxima vez que o Bardo abrir, cada tarefa em execução continua do último ponto que salvou, e a tentativa em que estava não conta nas novas tentativas. Uma tarefa que já tinha entregado trabalho a um provedor (um clipe sendo feito, um envio sendo processado) pergunta ao provedor como ficou, em vez de pagar duas vezes. Uma tarefa esperando nova tentativa, ou esperando a hora dela, continua esperando.

<a id="test"></a>
## Tarefas de teste

**Iniciar tarefa de teste** roda uma contagem de dez segundos que não faz nada e não custa nada, para testar progresso, cancelamento, nova tentativa e o Bardo fechando no meio. **Iniciar tarefa de teste com falha** falha de propósito, para você ver uma nova tentativa automática e uma falha.
