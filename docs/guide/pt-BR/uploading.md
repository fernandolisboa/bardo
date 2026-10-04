---
id: uploading
title: Envio e agendamento
group: publishing
place: projects/publish
tour: publish
---

# Envio e agendamento

A etapa [Publicação](bardo:go/projects/publish) transforma cada arquivo renderizado num post: escreve o texto do post para cada rede e então o envia a uma conta conectada do YouTube, do Instagram Reels ou do TikTok, ou o [exporta](exporting.md) para você postar à mão. Nada sai do seu computador antes de você confirmar uma revisão. [Mostre a etapa para mim](bardo:tour/publish).

<a id="networks"></a>
## Um post por rede

Cada [conta de rede](network-accounts.md) do canal é uma linha, com o @, o título ou a legenda do post, o estado e a última exportação. Escolha uma linha para ver o post dela no inspetor. Os números no topo contam as redes renderizadas e exportadas, dizem se os metadados estão escritos ou editados e quanto custou escrevê-los.

Uma rede precisa antes do arquivo da etapa [Render](render.md); até lá, a linha dela diz **Sem render**.

<a id="metadata"></a>
## Metadados e seus limites

**Escrever metadados** faz o Claude escrever o post de todas as redes de uma vez, a partir do roteiro, do canal e dos [padrões de metadados](network-accounts.md#metadata) de cada conta, com o template de metadados do projeto. Isso roda como tarefa e o custo vai para [Custos](bardo:go/costs); **Escrever de novo** substitui o texto de todas as redes, então pergunta antes quando você editou algum.

Edite o post de cada rede no inspetor. Cada campo conta contra o limite da rede, e um post acima de um limite não pode ser exportado nem enviado até você corrigir:

| Rede | Título | Descrição ou legenda | Tags |
| --- | --- | --- | --- |
| YouTube | 100 caracteres | 5.000 caracteres | 500 caracteres no total |
| TikTok | nenhum | 2.200 caracteres | como hashtags na legenda |
| Instagram Reels | nenhum | 2.200 caracteres | até 5 hashtags na legenda |
| X | nenhum | 280 caracteres | como hashtags no post |
| Kick | 100 caracteres | nenhuma | até 10 |

O YouTube também não aceita < nem > no título nem na descrição. O rodapé da conta e as hashtags são adicionados do jeito que a rede os recebe, e a contagem os inclui; **Como será postado**, embaixo dos campos, mostra o resultado. **Salvar** guarda suas edições e **Reverter** volta ao que estava salvo.

<a id="disclosure"></a>
## Conteúdo sintético

Quando a voz do narrador está marcada como clone ou voz sintética realista, cada rede pede que você diga isso, e o Bardo lembra você no post de cada rede:

- **YouTube**: "Conteúdo alterado ou sintético", que a revisão do envio marca para você.
- **Instagram**: o rótulo "Informações de IA", que a revisão marca para você.
- **TikTok**: o rótulo de conteúdo gerado por IA, que você ativa no app do TikTok; o lembrete da revisão aparece quando o rascunho chega.
- **X e Kick** não têm rótulo: diga isso no texto ou no título do post.

O arquivo de metadados de uma exportação diz onde ativar o rótulo em cada rede.

<a id="review"></a>
## A revisão do envio

Uma conta conectada oferece **Revisar envio**. O botão fica desligado, com o motivo embaixo, enquanto a conta não está conectada, o render está em andamento ou desatualizado, os metadados faltam ou passam dos limites da rede, ou o arquivo não atende aos requisitos da rede. A revisão mostra exatamente o que vai:

- o arquivo, a conta e o título, a descrição e as tags (ou a legenda) do jeito que a rede os recebe;
- suas escolhas: visibilidade e conteúdo para crianças no YouTube, a capa e Mostrar também no Feed no Instagram, o rótulo de conteúdo sintético;
- quando vai (veja [abaixo](#schedule));
- se o projeto já tem um post nessa rede, uma caixa para substituí-lo no Bardo (o post continua na rede).

Confirmar inicia o envio como tarefa, com o progresso aqui e em Tarefas. Se o render, o corte ou os metadados mudarem com a revisão aberta, o Bardo fecha a revisão e pede que você revise de novo. O guia de cada rede tem os detalhes: [YouTube](connect-youtube.md#upload), [Instagram](connect-instagram.md#upload), [TikTok](connect-tiktok.md#draft).

<a id="states"></a>
## Estados do envio

| Estado | Significa |
| --- | --- |
| Aguardando envio | Revisado; a tarefa ainda não começou |
| Enviando | Mandando o arquivo; **Parar** mantém o que a rede já tem, **Retomar** manda o resto |
| Tentando de novo | Uma tentativa falhou no caminho; o Bardo tenta de novo em instantes |
| Processando | A rede tem o arquivo e está trabalhando nele |
| Ainda processando | O Bardo parou de esperar pela rede; **Verificar de novo** mais tarde |
| Agendado | No YouTube, privado até o horário de publicação |
| Agendado no Bardo | Instagram: o Bardo posta no horário, com o Bardo aberto |
| Perdeu o horário | O Bardo estava fechado no horário; veja [Posts perdidos](missed-posts.md) |
| Acima do limite de publicação | Na fila até a rede aceitar mais posts |
| Enviado | Na rede, com o link |
| Mantido privado | O YouTube manteve privado: veja [Envios ficam privados até a auditoria](connect-youtube.md#private) |
| Rascunho no TikTok | Na caixa de entrada do TikTok, para você finalizar no app |
| Envio parado, Envio falhou | Parado por você ou recusado; **Retomar** ou **Tentar de novo** quando puder continuar |

<a id="schedule"></a>
## Agendamento

Cada rede agenda do seu jeito:

- O **YouTube** aceita um horário de publicação. Na revisão, escolha **Agendar** e digite a data e a hora, lidas no fuso horário do seu computador: o Bardo envia o vídeo como privado e o YouTube o torna público nesse horário sozinho, mesmo com o Bardo e o computador desligados. Até lá, **Mudar horário** e **Cancelar agendamento** mudam isso no YouTube. Mais em [Agendar no YouTube](connect-youtube.md#schedule).
- O **Instagram** não aceita horário de publicação de apps. Com **Agendar**, o Bardo envia o arquivo antes e publica o Reel ele mesmo nesse horário, então deixe o Bardo aberto e o computador ligado. Se estiver fechado, nada é postado e o Bardo pergunta o que fazer quando abrir: veja [Posts perdidos](missed-posts.md).
- O **TikTok** recebe um rascunho na sua caixa de entrada, nunca um post. Você cola a legenda, escolhe quem pode assistir e posta ou agenda no app do TikTok. O Bardo não agenda rascunhos.

Um horário que já passou é recusado quando você confirma.

<a id="export"></a>
## Exportar

**Exportar** grava uma pasta por rede escolhida com o arquivo renderizado e um arquivo de metadados para copiar, para postar à mão em qualquer uma das cinco redes. É o jeito de postar no X e na Kick, e funciona para as outras também. Veja [Exportação](exporting.md).

<a id="post"></a>
## A publicação

A seção **Publicação** acompanha o post da rede depois que ele está no ar:

- Um envio vincula o post sozinho.
- Para um post que você fez à mão, cole o link e escolha **Marcar como publicado**. **Trocar link** e **Desvincular** corrigem um link errado; desvincular remove os números dele no Bardo, e o post continua na rede.
- Um rascunho do TikTok mostra a legenda com **Copiar legenda**; depois de postar no app, marque como publicado com o link.

Posts vinculados têm os números em [Desempenho](performance-metrics.md): os números públicos do YouTube com uma chave da YouTube Data API, e os números próprios de cada conta conectada.
